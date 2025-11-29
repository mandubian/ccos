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
- **Uses Core**: For discovery
- **Adds**: Output schema introspection, RTFS generation
- **Removes**: Duplicate discovery logic

## Migration Checklist

- [ ] Create `mcp/core.rs` with unified discovery
- [ ] Add catalog integration to core
- [ ] Add marketplace integration to core
- [ ] Add caching layer
- [ ] Update `mcp.rs` to use core
- [ ] Update `mcp_discovery.rs` to use core
- [ ] Update `mcp_introspector.rs` to use core
- [ ] Remove duplicate code
- [ ] Update tests
- [ ] Update documentation

