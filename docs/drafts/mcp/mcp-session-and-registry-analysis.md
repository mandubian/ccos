# MCP Session & Registry Modules Analysis

## 1. `mcp_session.rs` (MCPSessionManager)

### Purpose
**Discovery-time session management** - Used during capability discovery/introspection

### Usage
- `mcp_introspector.rs` - Introspects MCP servers
- `mcp_discovery.rs` - Discovers tools from servers
- `mcp_session_handler.rs` - NOT used (different implementation)

### Characteristics
- **Async API** - Pure async/await
- **Ephemeral sessions** - Initialize → Use → Terminate (no pooling)
- **Discovery-focused** - Used for one-off tool discovery

### Verdict: ✅ **KEEP** - Needed for discovery

**Should be used BY unified core**, not replaced.

---

## 2. `mcp_registry_client.rs` (McpRegistryClient)

### Purpose
**MCP Registry API client** - Queries the official MCP registry (registry.modelcontextprotocol.io)

### Usage
- `missing_capability_resolver.rs` - Searches registry for missing capabilities
- `discovery/engine.rs` - Searches registry during discovery
- `planner/modular_planner/resolution/mcp.rs` - Has `registry_client` field (but may not use it)

### Characteristics
- **External API client** - Queries public MCP registry
- **Server discovery** - Finds which MCP servers exist
- **Separate concern** - Not about discovering tools FROM servers, but finding WHICH servers to query

### Verdict: ✅ **KEEP** - Needed for registry search

**Should be used BY unified core** when searching for servers, not replaced.

---

## 3. `mcp_session_handler.rs` (MCPSessionHandler)

### Purpose
**Runtime execution session management** - Used during capability execution

### Usage
- `SessionPoolManager` - Implements `SessionHandler` trait
- Runtime capability execution (not discovery)

### Characteristics
- **Blocking API** - Wraps async in `block_in_place` (for RTFS runtime)
- **Session pooling** - Reuses sessions across calls
- **Execution-focused** - Used when actually calling capabilities

### Verdict: ✅ **KEEP SEPARATE** - Different purpose

**NOT part of discovery** - This is for runtime execution, not discovery.

---

## Architecture After Unification

```
┌─────────────────────────────────────────┐
│   Unified MCP Discovery Core            │
│   (mcp/core.rs)                         │
│                                         │
│   Uses:                                 │
│   ├─ MCPSessionManager                  │
│   │  (mcp_session.rs)                   │
│   │  └─> For discovery sessions         │
│   │                                     │
│   └─ McpRegistryClient                  │
│      (mcp_registry_client.rs)            │
│      └─> For finding servers            │
└─────────────────────────────────────────┘
         │
         │ (separate)
         ▼
┌─────────────────────────────────────────┐
│   Runtime Execution                     │
│                                         │
│   MCPSessionHandler                     │
│   (mcp_session_handler.rs)              │
│   └─> For capability execution          │
└─────────────────────────────────────────┘
```

## Summary

| Module | Purpose | Keep? | Location |
|--------|---------|-------|----------|
| `mcp_session.rs` | Discovery sessions | ✅ Yes | Use by unified core |
| `mcp_registry_client.rs` | Registry search | ✅ Yes | Use by unified core |
| `mcp_session_handler.rs` | Runtime execution | ✅ Yes | Keep separate (different purpose) |

## Recommendation

1. **Keep all three** - They serve different purposes
2. **Unified core uses** `mcp_session.rs` and `mcp_registry_client.rs`
3. **Move to `mcp/` module** for better organization:
   ```
   mcp/
   ├── core.rs              # Unified discovery service
   ├── session.rs           # Discovery sessions (from synthesis/)
   ├── registry.rs          # Registry client (from synthesis/)
   ├── types.rs             # Shared types
   └── cache.rs             # Caching layer
   ```
4. **Keep `mcp_session_handler.rs`** in `capabilities/` (runtime execution concern)

