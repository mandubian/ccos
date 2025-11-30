# MCP Storage Layers Explained

This document clarifies the roles and differences between three storage/caching mechanisms in the MCP discovery system.

## Overview

There are three distinct layers with different purposes:

1. **MCPRegistry** (`MCPRegistryClient`) - Server Discovery
2. **MCPCache** - Performance Optimization
3. **capabilities/discovered/** - Persistent Capability Storage

---

## 1. MCPRegistry (`MCPRegistryClient`)

### Purpose
**Server Discovery** - Finds which MCP servers exist and are available

### What It Does
- Queries the **official MCP Registry API** (`registry.modelcontextprotocol.io`)
- Returns metadata about available MCP servers (name, description, endpoints, packages)
- Helps answer: "What MCP servers are available?" or "Which server provides capability X?"

### Data Stored
- Server metadata (name, description, version, repository, packages, remotes)
- Server endpoints and configuration
- **NOT** tool schemas or capabilities

### Lifetime
- **Ephemeral** - Results are not cached (always queries live registry)
- Used when searching for servers to connect to

### Example Use Case
```rust
// "I need a GitHub integration - which MCP servers provide that?"
let servers = registry_client.search_servers("github").await?;
// Returns: List of server metadata (not tools)
```

### Location
- `ccos/src/mcp/registry.rs`
- External API: `https://registry.modelcontextprotocol.io`

---

## 2. MCPCache

### Purpose
**Performance Optimization** - Avoids redundant queries to MCP servers

### What It Does
- Caches the **results of tool discovery** from individual MCP servers
- Stores `DiscoveredMCPTool` objects (parsed tool schemas)
- Checks cache before querying a server's `tools/list` endpoint

### Data Stored
- `Vec<DiscoveredMCPTool>` per server endpoint
- Includes: tool names, descriptions, input/output schemas (parsed from JSON Schema)
- Timestamp for TTL management

### Lifetime
- **Short-term** - Default TTL: 24 hours
- **In-memory** (fast) + optional **file-based** (persists across restarts)
- Automatically expires and refreshes

### Example Use Case
```rust
// First call: Queries server, caches result
let tools = service.discover_tools(&config, &options).await?; // Hits server

// Second call (within 24h): Returns cached result
let tools = service.discover_tools(&config, &options).await?; // Returns from cache
```

### Location
- `ccos/src/mcp/cache.rs`
- Storage: In-memory HashMap + optional `cache_dir/` files

---

## 3. capabilities/discovered/ RTFS Files

### Purpose
**Persistent Capability Storage** - Long-term storage for reloading capabilities

### What It Does
- Exports discovered capabilities to **RTFS module files**
- Stores complete capability definitions with schemas and implementations
- Allows reloading capabilities without re-discovering from servers

### Data Stored
- Complete `(capability ...)` RTFS expressions
- Input/output schemas (as RTFS TypeExpr)
- Implementation code (RTFS functions)
- Server metadata (for reference)

### Lifetime
- **Permanent** - Files persist until manually deleted
- **Not automatically refreshed** - Must be explicitly re-exported
- Used for **offline capability loading**

### Example Use Case
```rust
// During discovery: Auto-export to RTFS
let options = DiscoveryOptions {
    export_to_rtfs: true,
    export_directory: Some("capabilities/discovered".to_string()),
    // ...
};
service.discover_and_export_tools(&config, &options).await?;
// Creates: capabilities/discovered/mcp/github/capabilities.rtfs

// Later: Reload from file (no server query needed)
marketplace.load_capabilities_from_rtfs("capabilities/discovered/mcp/github/capabilities.rtfs")?;
```

### Location
- `capabilities/discovered/mcp/<server_name>/capabilities.rtfs`
- Format: RTFS module file with `(do ...)` block containing multiple capabilities

---

## Comparison Table

| Aspect | MCPRegistry | MCPCache | capabilities/discovered/ |
|--------|-------------|----------|-------------------------|
| **Purpose** | Find servers | Cache tool discovery | Store capabilities |
| **Data Type** | Server metadata | `DiscoveredMCPTool[]` | RTFS capability definitions |
| **Lifetime** | Ephemeral | 24h TTL | Permanent |
| **Storage** | None (API queries) | Memory + optional files | RTFS files |
| **Refresh** | Always live | Auto-expires | Manual re-export |
| **Use Case** | "Which servers exist?" | "Avoid redundant queries" | "Reload capabilities offline" |

---

## Data Flow

```
1. MCPRegistry
   └─> "Find GitHub server" → Returns: Server metadata (endpoint, config)

2. MCPDiscoveryService.discover_tools()
   ├─> Check MCPCache
   │   └─> Cache hit? Return cached tools
   │   └─> Cache miss? Query server's tools/list
   │       └─> Parse tools → Store in cache
   │
   └─> Convert to CapabilityManifest
       └─> Register in marketplace
       └─> Export to capabilities/discovered/ (if enabled)

3. capabilities/discovered/
   └─> RTFS files can be loaded later without server queries
```

---

## When to Use Each

### Use MCPRegistry when:
- Searching for servers that provide specific capabilities
- Discovering new MCP servers to integrate
- Finding server endpoints and configuration

### Use MCPCache when:
- You want to avoid redundant server queries
- Performance optimization is needed
- Short-term caching is acceptable (24h default)

### Use capabilities/discovered/ when:
- You want to persist capabilities for offline use
- You need to reload capabilities without server access
- You want human-readable RTFS definitions
- You're doing capability version control or backup

---

## Summary

- **MCPRegistry** = "What servers exist?" (server discovery)
- **MCPCache** = "Don't query the same server twice" (performance)
- **capabilities/discovered/** = "Save capabilities for later" (persistence)

All three work together:
1. Registry helps find servers
2. Cache speeds up repeated discovery
3. RTFS files enable offline capability loading

