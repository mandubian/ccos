# MCP Discovery: Next Steps

## ✅ Completed

1. **Unified MCP Discovery Service** - `MCPDiscoveryService` consolidates all discovery logic
2. **Moved `DiscoveredMCPTool` to `mcp/types.rs`** - Core type now in unified module
3. **Automatic RTFS Export** - Discovered capabilities auto-export to `capabilities/discovered/`
4. **Storage Layers Documented** - Clear explanation of Registry vs Cache vs RTFS files
5. **Output Schema Introspection (Option 1)** - Implemented introspection of output schemas by calling tools with safe inputs
6. **Test & Validate Unified Service (Option 6)** - Comprehensive test suite with 26 tests in `ccos/tests/mcp_discovery_tests.rs`
7. **Rate Limiting & Retry Policies (Option 3)** - Token bucket rate limiting with exponential backoff retry in `ccos/src/mcp/rate_limiter.rs`

---

## 🎯 Next Steps (Choose One)

### ~~Option 1: Output Schema Introspection~~ ✅ COMPLETED
**Priority**: Medium | **Effort**: 2-3 hours | **Impact**: High

**Status**: Implemented in `ccos/src/mcp/core.rs`. The `discover_tools()` method now:
- Uses `MCPIntrospector::introspect_output_schema()` when `introspect_output_schemas` option is enabled
- Calls tools with minimal/safe inputs to infer output schemas
- Falls back gracefully when introspection fails

**Files Modified**:
- `ccos/src/mcp/core.rs` - Added introspection call in tool discovery loop
- `ccos/examples/mcp_discovery_demo.rs` - Updated demo with introspection example

---

### ~~Option 2: Load RTFS Capabilities on Startup~~ ✅ COMPLETED
**Priority**: Medium | **Effort**: 3-4 hours | **Impact**: Medium

**Status**: Implemented in `ccos/src/capability_marketplace/marketplace.rs`.

**Features**:
- `load_discovered_capabilities()` - Loads from `capabilities/discovered/` on startup
- `import_capabilities_from_rtfs_dir_recursive()` - Recursively scans directories for `.rtfs` files
- `import_single_rtfs_file()` - Loads individual files with duplicate detection
- Integrated into `bootstrap()` - Auto-loads discovered capabilities during marketplace initialization
- Handles duplicate capabilities (skips same version, updates if different version)

**Directory Structure Supported**:
```text
capabilities/discovered/
├── mcp/
│   ├── github/
│   │   └── capabilities.rtfs
│   └── slack/
│       └── capabilities.rtfs
└── other/
    └── capabilities.rtfs
```

**Tests Added** (6 new tests in `mcp_discovery_tests.rs`):
- `test_load_discovered_capabilities_empty_dir`
- `test_load_discovered_capabilities_nonexistent_dir`
- `test_load_discovered_capabilities_flat_dir`
- `test_load_discovered_capabilities_recursive`
- `test_load_discovered_capabilities_ignores_non_rtfs`
- `test_import_single_rtfs_file_duplicate_handling`

---

### ~~Option 3: Rate Limiting & Retry Policies~~ ✅ COMPLETED
**Priority**: High | **Effort**: 4-6 hours | **Impact**: High

**Status**: Implemented in `ccos/src/mcp/rate_limiter.rs`. Features include:
- **Token bucket rate limiting** with configurable requests/second and burst size
- **Per-server rate limiting** for different limits on different servers
- **Exponential backoff retry** with configurable max retries, delays, and jitter
- **Retryable error detection** for 429, 503, and timeout errors

**Files Created/Modified**:
- `ccos/src/mcp/rate_limiter.rs` (NEW) - Complete rate limiting implementation
- `ccos/src/mcp/types.rs` - Added `RateLimitConfig` and `RetryPolicy` structs
- `ccos/src/mcp/core.rs` - Integrated rate limiter and retry loop in `discover_tools()`
- `ccos/src/mcp/mod.rs` - Exported new types
- `ccos/tests/mcp_discovery_tests.rs` - Added tests for new fields

---

### ~~Option 4: MCP Registry Integration~~ ✅ COMPLETED
**Priority**: Medium | **Effort**: 4-5 hours | **Impact**: Medium

**Status**: Implemented registry integration in `MCPDiscoveryService` and `MCPCache`.

**Features Added**:

1. **Registry Search Methods** (in `ccos/src/mcp/core.rs`):
   - `search_registry_for_capability(query, use_cache)` - Search MCP registry with caching
   - `find_servers_for_capability(capability_name, options)` - Local-first search with registry fallback
   - `discover_from_registry(query, options)` - High-level discovery from registry-found servers
   - `registry_server_to_config(server)` - Convert registry server to MCPServerConfig
   - `registry_client()` - Access the underlying registry client

2. **Registry Search Caching** (in `ccos/src/mcp/cache.rs`):
   - `get_registry_search(query)` - Retrieve cached registry search results
   - `store_registry_search(query, servers)` - Cache registry search results
   - 1-hour TTL for registry cache (vs 24-hour for tool cache)
   - File persistence support for registry cache

**Tests Added** (8 new tests in `mcp_discovery_tests.rs`):
- `test_registry_cache_store_and_retrieve`
- `test_registry_cache_miss_for_unknown_query`
- `test_registry_cache_with_file_persistence`
- `test_registry_cache_clear_includes_registry`
- `test_discovery_service_registry_client_accessible`
- `test_find_servers_for_capability_local_first`
- `test_registry_server_to_config_with_remotes`
- `test_registry_server_without_remotes`

**Usage Example**:
```rust
let service = MCPDiscoveryService::new();
let options = DiscoveryOptions { use_cache: true, ..Default::default() };

// Search registry and discover tools from matching servers
let results = service.discover_from_registry("github", &options).await?;
for (server_config, tools) in results {
    println!("Found {} tools from {}", tools.len(), server_config.name);
}
```

---

### Option 5: Capability Versioning & Updates 📦 (Maintenance)
**Priority**: Low | **Effort**: 6-8 hours | **Impact**: Medium

**What**: Add versioning support for discovered capabilities and ability to update them.

**Current State**:
- Capabilities are discovered and stored
- No version tracking
- No update mechanism

**Implementation**:
- Add version metadata to capabilities
- Compare versions when reloading
- Add `update_capability()` method
- Handle breaking changes

**Files to Modify**:
- `ccos/src/capability_marketplace/types.rs` (add version fields)
- `ccos/src/mcp/core.rs` (version comparison)
- `ccos/src/capability_marketplace/marketplace.rs` (update logic)

**Benefits**:
- Track capability changes
- Handle schema evolution
- Better maintenance

---

### ~~Option 6: Test & Validate Unified Service~~ ✅ COMPLETED
**Priority**: High | **Effort**: 3-4 hours | **Impact**: High

**Status**: Comprehensive test suite created with 26 tests in `ccos/tests/mcp_discovery_tests.rs`.

**Test Coverage**:
- Cache tests (memory and file-based, TTL, clear, sanitization)
- Discovery options tests (defaults, custom configuration)
- Discovered tool tests (serialization, complex schemas)
- Error handling tests (missing directories, invalid JSON)
- Discovery service tests (creation, marketplace/catalog integration)
- Async tests (cache usage, capability registration)
- Integration tests (RTFS export, full discovery workflow)

**Files Created**:
- `ccos/tests/mcp_discovery_tests.rs` - Comprehensive test suite
- `ccos/examples/mcp_discovery_demo.rs` - Enhanced demo with rate limiting

---

### Option 7: Performance Optimization ⚡ (Speed)
**Priority**: Medium | **Effort**: 4-5 hours | **Impact**: Medium

**What**: Optimize discovery performance (parallel discovery, better caching, connection pooling).

**Current State**:
- Sequential discovery
- Basic caching
- New session per discovery

**Implementation**:
- Parallel discovery of multiple servers
- Connection pooling for session manager
- Better cache warming
- Lazy loading of schemas

**Files to Modify**:
- `ccos/src/mcp/core.rs` (parallel discovery)
- `ccos/src/mcp/discovery_session.rs` (connection pooling)
- `ccos/src/mcp/cache.rs` (cache warming)

**Benefits**:
- Faster discovery
- Better resource utilization
- Scalability

---

### Option 8: Complete MCP Discovery Migration 🔄 (Code Cleanup)
**Priority**: High | **Effort**: 2-3 hours | **Impact**: High

**What**: Complete the migration from `mcp_discovery.rs` legacy code to `mcp/core.rs` unified service.

**Current State**:
- `MCPDiscoveryService` in `mcp/core.rs` is the unified, complete implementation
- `MCPDiscoveryProvider` in `mcp_discovery.rs` has optional `unified_service` field
- By default, `unified_service` is `None`, so legacy code path is used
- Legacy implementation duplicates discovery logic

**Problem**:
- Redundant code paths (legacy vs unified)
- Default behavior uses legacy code instead of unified service
- Migration incomplete - both implementations coexist

**Implementation**:
- Make `MCPDiscoveryProvider` always use `MCPDiscoveryService` (remove `unified_service` optional)
- Remove legacy `discover_raw_tools()` and related methods from `mcp_discovery.rs`
- Keep `MCPDiscoveryProvider` as thin `CapabilityDiscovery` adapter that delegates to `MCPDiscoveryService`
- Update `MCPDiscoveryProvider::new()` to create and use `MCPDiscoveryService` internally
- Remove `with_unified_service()` method (no longer needed)

**Files to Modify**:
- `ccos/src/capability_marketplace/mcp_discovery.rs` (remove legacy code, always use unified service)
- `ccos/src/mcp/core.rs` (ensure all needed methods are public)
- Update any code that calls `with_unified_service()` (should be minimal)

**Benefits**:
- Eliminates code duplication
- Single source of truth for MCP discovery
- Better maintainability
- Consistent behavior (always uses caching, rate limiting, etc.)

---

## 📊 Recommendation Matrix

| Option | Priority | Effort | Impact | Dependencies |
|--------|----------|--------|--------|--------------|
| **1. Output Schema Introspection** | Medium | Low | High | None |
| **2. Load RTFS on Startup** | Medium | Medium | Medium | Option 1 (nice to have) |
| **3. Rate Limiting** | High | Medium | High | None |
| **4. Registry Integration** | Medium | Medium | Medium | None |
| **5. Versioning** | Low | High | Medium | Option 2 |
| **6. Testing** | High | Medium | High | None |
| **7. Performance** | Medium | Medium | Medium | Option 6 (test first) |
| **8. Complete MCP Discovery Migration** | High | Low | High | None |

---

## 🎯 Suggested Order

1. **Option 8: Complete MCP Discovery Migration** (High priority, code cleanup, eliminates redundancy)
2. **Option 6: Testing** (High priority, validates everything)
3. **Option 1: Output Schema Introspection** (Quick win, high impact)
4. **Option 3: Rate Limiting** (High priority, reliability)
5. **Option 2: Load RTFS on Startup** (Infrastructure improvement)
6. **Option 4: Registry Integration** (Discovery enhancement)
7. **Option 7: Performance** (Optimization)
8. **Option 5: Versioning** (Future maintenance)

---

## 💡 Quick Wins (Can Do Now)

- **Option 8**: Complete MCP discovery migration (remove legacy code, always use unified service)
- **Option 1**: Output schema introspection (code already exists, just needs wiring)
- **Option 6**: Add a few more test cases to existing example

---

## 🔮 Future Considerations

- **JIT Polyglot Generation**: See `docs/drafts/future-jit-polyglot-generation.md`
- **Discovery Hints & Re-planning**: See `docs/drafts/discovery_hints_replanning.md`
- **Schema-Adaptive Discovery**: See `docs/drafts/schema-adaptive-capability-discovery.md`

