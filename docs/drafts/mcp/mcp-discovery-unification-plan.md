# MCP Discovery Unification Plan

## Current State Analysis

### Three MCP Modules with Overlapping Responsibilities

#### 1. `ccos/src/planner/modular_planner/resolution/mcp.rs`
**Purpose**: Intent Resolution Strategy
- **Role**: Maps `SubIntent` → `ResolvedCapability` for the modular planner
- **Key Components**:
  - `McpDiscovery` trait (async trait for discovery operations)
  - `RuntimeMcpDiscovery` (implements `McpDiscovery`)
  - `McpResolution` (implements `ResolutionStrategy`)
- **Features**:
  - Domain hint → MCP server mapping
  - Tool scoring against intents (keyword + embedding-based)
  - File-based caching (`{server}_tools.json`)
  - In-memory tool cache
  - Automatic tool registration in marketplace
- **Dependencies**: Uses `MCPDiscoveryProvider` from `mcp_discovery.rs` internally

#### 2. `ccos/src/capability_marketplace/mcp_discovery.rs`
**Purpose**: Marketplace Capability Discovery
- **Role**: Implements `CapabilityDiscovery` trait for marketplace
- **Key Components**:
  - `MCPDiscoveryProvider` (implements `CapabilityDiscovery`)
  - `MCPServerConfig` (server configuration)
  - `MCPTool` (raw tool definition from server)
  - RTFS module save/load (`save_rtfs_capabilities`, `load_rtfs_capabilities`)
- **Features**:
  - Discovers tools and resources from MCP servers
  - Converts tools to `CapabilityManifest`
  - RTFS module persistence (multiple capabilities in one file)
  - Effects derivation from metadata
  - Output schema introspection (via `MCPIntrospector`)
- **Dependencies**: Uses `MCPIntrospector` for introspection

#### 3. `ccos/src/synthesis/mcp_introspector.rs`
**Purpose**: MCP Tool Introspection & RTFS Generation
- **Role**: Low-level introspection and RTFS code generation
- **Key Components**:
  - `MCPIntrospector` (main struct)
  - `DiscoveredMCPTool` (parsed tool with schemas)
  - `MCPIntrospectionResult` (server introspection result)
- **Features**:
  - Parses MCP tools from JSON-RPC responses
  - Converts JSON Schema → RTFS `TypeExpr`
  - Output schema introspection (calls tools with safe inputs)
  - Generates RTFS capability strings
  - Saves individual capabilities to RTFS files
- **Dependencies**: Uses `MCPSessionManager` for session management

## Overlap & Redundancy

### Shared Functionality
1. **Tool Discovery**: All three discover tools via `tools/list` MCP call
2. **Session Management**: All use `MCPSessionManager` (though sometimes create new instances)
3. **Schema Conversion**: All convert JSON Schema → RTFS `TypeExpr`
4. **Manifest Creation**: All create `CapabilityManifest` from MCP tools
5. **Caching**: `mcp.rs` has file caching; others may benefit from it

### Redundant Code
- Multiple implementations of tool discovery logic
- Duplicate session management setup
- Similar JSON Schema → TypeExpr conversion (though `mcp_introspector.rs` is most complete)
- Overlapping manifest creation logic

## Proposed Unified Architecture

### Core Layer: `ccos/src/mcp/core.rs` (NEW)

A single, unified MCP discovery API that all modules use:

```rust
/// Unified MCP Discovery Service
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
    /// Discover tools from a server
    pub async fn discover_tools(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>>;
    
    /// Discover resources from a server
    pub async fn discover_resources(
        &self,
        server_config: &MCPServerConfig,
    ) -> RuntimeResult<Vec<serde_json::Value>>;
    
    /// Get server config for a domain hint
    pub fn get_server_for_domain(&self, domain: &DomainHint) -> Option<MCPServerConfig>;
    
    /// List all known servers
    pub fn list_known_servers(&self) -> Vec<MCPServerConfig>;
    
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
}
```

### Specialized Layers (Use Core)

1. **Resolution Layer** (`mcp.rs`): Uses core for discovery, adds intent scoring
2. **Marketplace Layer** (`mcp_discovery.rs`): Uses core for discovery, adds RTFS persistence
3. **Introspection Layer** (`mcp_introspector.rs`): Uses core for discovery, adds schema introspection

## Migration Strategy

### Phase 1: Extract Core
1. Create `ccos/src/mcp/core.rs` with unified discovery logic
2. Move shared code from all three modules to core
3. Add caching layer to core (file + memory)

### Phase 2: Refactor Modules
1. Update `mcp.rs` to use `MCPDiscoveryService`
2. Update `mcp_discovery.rs` to use `MCPDiscoveryService`
3. Update `mcp_introspector.rs` to use `MCPDiscoveryService`

### Phase 3: Consolidate
1. Remove duplicate code
2. Ensure single source of truth for:
   - Tool discovery
   - Schema conversion
   - Manifest creation
   - Session management

## Benefits

1. **Single Source of Truth**: One implementation of MCP discovery logic
2. **Consistent Caching**: All modules benefit from unified cache
3. **Easier Maintenance**: Fix bugs in one place
4. **Better Testing**: Test core once, all modules benefit
5. **Performance**: Shared session pool, better resource utilization

