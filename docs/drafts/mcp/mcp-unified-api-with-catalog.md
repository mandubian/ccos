# Unified MCP Discovery API (Including Catalog Integration)

## Overview

The unified MCP discovery API consolidates discovery logic from three modules and integrates with the catalog service for automatic indexing.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│         Unified MCP Discovery Service (Core)            │
│  ┌──────────────────────────────────────────────────┐  │
│  │  - Tool Discovery (with caching)                 │  │
│  │  - Schema Conversion (JSON Schema → RTFS)        │  │
│  │  - Manifest Creation                             │  │
│  │  - Session Management (shared pool)              │  │
│  └──────────────────────────────────────────────────┘  │
│                          │                               │
│         ┌────────────────┼────────────────┐            │
│         │                 │                │            │
│         ▼                 ▼                ▼            │
│  ┌──────────┐    ┌──────────────┐  ┌──────────┐      │
│  │Marketplace│    │   Catalog    │  │  Cache   │      │
│  │(optional) │    │  (optional)  │  │(optional)│      │
│  └──────────┘    └──────────────┘  └──────────┘      │
└─────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  Resolution  │    │  Marketplace │    │ Introspector│
│   Strategy   │    │   Discovery  │    │   (RTFS)    │
│  (mcp.rs)    │    │(mcp_discovery)│    │(mcp_introsp)│
└──────────────┘    └──────────────┘    └──────────────┘
```

## Catalog Integration

### Current Usage

1. **Resolution Strategy** (`mcp.rs`):
   - When a tool is registered via `register_tool()`, it indexes in catalog with `CatalogSource::Discovered`
   - See: `planner/modular_planner/resolution/mcp.rs:194-197`

2. **Marketplace** (`marketplace.rs`):
   - Has `index_capability_in_catalog()` method
   - Automatically indexes when capabilities are registered
   - See: `capability_marketplace/marketplace.rs:333-343`

### Unified API Integration

The unified `MCPDiscoveryService` will:

1. **Accept Optional Catalog**: 
   ```rust
   pub fn with_catalog(mut self, catalog: Arc<CatalogService>) -> Self {
       self.catalog = Some(catalog);
       self
   }
   ```

2. **Auto-Index on Registration**:
   ```rust
   pub async fn register_capability(
       &self,
       manifest: &CapabilityManifest,
   ) -> RuntimeResult<()> {
       // Register in marketplace (if provided)
       if let Some(ref marketplace) = self.marketplace {
           marketplace.register_capability_manifest(manifest.clone()).await?;
       }
       
       // Index in catalog (if provided)
       if let Some(ref catalog) = self.catalog {
           catalog.register_capability(manifest, CatalogSource::Discovered);
       }
       
       Ok(())
   }
   ```

3. **Benefits**:
   - All discovered MCP capabilities automatically searchable
   - Consistent source tagging (`CatalogSource::Discovered`)
   - No need for each module to manually index
   - Single point of control for catalog integration

## Module Responsibilities After Unification

### Core (`mcp/core.rs`)
- **Discovery**: Tool/resource discovery from MCP servers
- **Conversion**: JSON Schema → RTFS TypeExpr
- **Registration**: Marketplace + Catalog (if provided)
- **Caching**: Shared file + memory cache

### Resolution Strategy (`mcp.rs`)
- **Uses Core**: For discovery
- **Adds**: Intent scoring, domain hint mapping
- **Removes**: Duplicate discovery logic

### Marketplace Discovery (`mcp_discovery.rs`)
- **Uses Core**: For discovery
- **Adds**: RTFS module persistence
- **Removes**: Duplicate discovery logic

### Introspector (`mcp_introspector.rs`)
- **Used By Core**: `MCPDiscoveryService` contains an `MCPIntrospector` instance
- **Adds**: Output schema introspection, RTFS code generation
- **Note**: Remains as a specialized module for RTFS generation

## Migration Checklist

- [x] Create `mcp/core.rs` with unified discovery
- [x] Add catalog integration to core
- [x] Add marketplace integration to core
- [x] Add caching layer
- [x] Update `mcp.rs` to use core
- [x] Update `mcp_discovery.rs` to use core
- [x] Add rate limiting layer
- [x] Add domain/category inference for discovered capabilities
- [ ] Update `mcp_introspector.rs` to optionally use core for discovery (currently standalone)
- [ ] Remove duplicate code in introspector
- [x] Update tests
- [x] Update documentation

## Remaining Optional Work

### MCPIntrospector Integration (Low Priority)
The `MCPIntrospector` in `synthesis/introspection/` could optionally delegate discovery to 
`MCPDiscoveryService`, but currently works fine standalone. The introspector adds specialized 
RTFS generation that doesn't need to be in the unified service.

### Domain/Category Enhancement
Recently added automatic domain/category inference:
- Domains inferred from server name (e.g., "github" from "modelcontextprotocol/github")
- Sub-domains inferred from tool name (e.g., "issues" from "list_issues")
- Categories inferred from action patterns (e.g., "crud.read" from "list_*")


