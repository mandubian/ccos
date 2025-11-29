# MCP Discovery vs Catalog: Clarification

## They Serve Different Purposes

### `mcp_discovery.rs` (CapabilityDiscovery)
**Purpose**: **Discovery** - Finds capabilities from MCP servers
- Implements `CapabilityDiscovery` trait
- Added to marketplace's `discovery_agents` list
- During marketplace bootstrap, calls `discover()` to populate marketplace
- **Discovers** capabilities that don't exist yet

### `catalog` (CatalogService)
**Purpose**: **Indexing/Search** - Makes existing capabilities searchable
- Indexes capabilities that are **already in the marketplace**
- Provides keyword and semantic search
- Does NOT discover - it indexes what's already there
- **Indexes** capabilities that already exist

## Flow

```
1. Marketplace Bootstrap
   └─> Calls discovery_agents.discover()
       └─> mcp_discovery.rs discovers MCP tools
           └─> Adds to marketplace

2. Catalog Indexing (separate step)
   └─> catalog.ingest_marketplace()
       └─> Indexes all capabilities in marketplace
           └─> Makes them searchable
```

## Are They Redundant?

**No, but the architecture could be cleaner:**

### Current State
- `mcp_discovery.rs` = Full discovery implementation
- `catalog` = Separate indexing service
- Both operate on marketplace

### With Unified Core
- **Unified Core** (`mcp/core.rs`) = Actual discovery logic
- **`mcp_discovery.rs`** = Thin `CapabilityDiscovery` adapter (delegates to core)
- **`catalog`** = Indexes marketplace (unchanged)

## Proposed Architecture

```
┌─────────────────────────────────────────┐
│     Unified MCP Discovery Core          │
│     (mcp/core.rs)                       │
│  - Tool discovery                       │
│  - Schema conversion                    │
│  - Manifest creation                    │
└─────────────────────────────────────────┘
         ▲                    ▲
         │                    │
    ┌────┴────┐         ┌─────┴─────┐
    │         │         │           │
┌───▼───┐ ┌──▼──┐  ┌───▼──┐  ┌────▼────┐
│mcp.rs │ │mcp_ │  │Market│  │ Catalog │
│(resol)│ │disc │  │place │  │(indexes)│
│       │ │(adap│  │      │  │         │
│       │ │ter) │  │      │  │         │
└───────┘ └─────┘  └──────┘  └─────────┘
```

### `mcp_discovery.rs` as Thin Adapter

```rust
// Thin adapter that implements CapabilityDiscovery trait
pub struct MCPDiscoveryProvider {
    core: Arc<MCPDiscoveryService>,  // Uses unified core
    config: MCPServerConfig,
}

#[async_trait]
impl CapabilityDiscovery for MCPDiscoveryProvider {
    async fn discover(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Delegate to unified core
        let tools = self.core.discover_tools(&self.config, &DiscoveryOptions::default()).await?;
        
        // Convert to manifests
        let manifests: Vec<_> = tools.iter()
            .map(|tool| self.core.tool_to_manifest(tool, &self.config))
            .collect();
        
        Ok(manifests)
    }
}
```

## Benefits

1. **Single Source of Truth**: All discovery logic in unified core
2. **Thin Adapters**: `mcp_discovery.rs` becomes a simple trait adapter
3. **Clear Separation**: 
   - Core = Discovery logic
   - Adapter = Marketplace integration
   - Catalog = Search/indexing (separate concern)

## Answer to "Is mcp_discovery redundant with catalog?"

**No, they're complementary:**
- `mcp_discovery` = **Discovers** capabilities (adds to marketplace)
- `catalog` = **Indexes** capabilities (makes marketplace searchable)

But `mcp_discovery.rs` should be a **thin adapter** around the unified core, not a full implementation.

