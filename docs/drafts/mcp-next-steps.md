# MCP Discovery: Next Steps

## ✅ Completed

1. **Unified MCP Discovery Service** - `MCPDiscoveryService` consolidates all discovery logic
2. **Moved `DiscoveredMCPTool` to `mcp/types.rs`** - Core type now in unified module
3. **Automatic RTFS Export** - Discovered capabilities auto-export to `capabilities/discovered/`
4. **Storage Layers Documented** - Clear explanation of Registry vs Cache vs RTFS files

---

## 🎯 Next Steps (Choose One)

### Option 1: Output Schema Introspection ⚡ (Quick Win)
**Priority**: Medium | **Effort**: 2-3 hours | **Impact**: High

**What**: Implement the TODO in `mcp/core.rs` to actually introspect output schemas by calling tools with safe inputs.

**Current State**:
```rust
// TODO: Implement output schema introspection by calling tool with safe inputs
let output_schema = if options.introspect_output_schemas {
    None  // Not implemented yet
} else {
    None
};
```

**Implementation**:
- Use `MCPIntrospector::introspect_output_schema()` (already exists!)
- Call tool with minimal/safe inputs
- Parse response to infer output schema
- Handle errors gracefully (some tools may require auth)

**Files to Modify**:
- `ccos/src/mcp/core.rs` (line ~202)
- Test in `ccos/examples/test_unified_mcp_discovery.rs`

**Benefits**:
- Better capability schemas = better plan validation
- Enables data flow adapters (future work)
- Improves capability matching accuracy

---

### Option 2: Load RTFS Capabilities on Startup 🔄 (Infrastructure)
**Priority**: Medium | **Effort**: 3-4 hours | **Impact**: Medium

**What**: Add automatic loading of RTFS capabilities from `capabilities/discovered/` when marketplace initializes.

**Current State**:
- Capabilities are exported to RTFS files
- But not automatically reloaded on startup

**Implementation**:
- Add `load_capabilities_from_rtfs_dir()` to `CapabilityMarketplace`
- Scan `capabilities/discovered/mcp/*/capabilities.rtfs`
- Parse RTFS files and register capabilities
- Handle duplicates (skip or update?)

**Files to Modify**:
- `ccos/src/capability_marketplace/marketplace.rs`
- `ccos/src/capability_marketplace/config_mcp_discovery.rs` (or new loader)
- Add to marketplace initialization

**Benefits**:
- Offline capability loading
- Faster startup (no server queries needed)
- Version control for capabilities

---

### Option 3: Rate Limiting & Retry Policies 🛡️ (Reliability)
**Priority**: High | **Effort**: 4-6 hours | **Impact**: High

**What**: Add rate limiting and retry logic to MCP discovery to handle API limits gracefully.

**Current State**:
- No rate limiting
- No retry logic
- Fails immediately on errors

**Implementation**:
- Add rate limiter (token bucket or similar)
- Implement exponential backoff retry
- Handle 429 (Too Many Requests) responses
- Add configurable retry policies

**Files to Modify**:
- `ccos/src/mcp/core.rs` (add rate limiter)
- `ccos/src/mcp/discovery_session.rs` (retry logic)
- Add `RateLimitConfig` to `DiscoveryOptions`

**Benefits**:
- More reliable discovery
- Better handling of API limits
- Prevents overwhelming servers

---

### Option 4: MCP Registry Integration 🔍 (Discovery Enhancement)
**Priority**: Medium | **Effort**: 4-5 hours | **Impact**: Medium

**What**: Better integration of `MCPRegistryClient` into the unified service for server discovery.

**Current State**:
- `MCPRegistryClient` exists but not fully integrated
- `MCPDiscoveryService` has registry client but doesn't use it much

**Implementation**:
- Add `discover_servers_for_capability()` method
- Search registry when domain hint doesn't match known servers
- Auto-configure servers from registry results
- Cache registry search results

**Files to Modify**:
- `ccos/src/mcp/core.rs` (add registry search methods)
- `ccos/src/mcp/registry.rs` (enhance search)
- `ccos/src/planner/modular_planner/resolution/mcp.rs` (use registry search)

**Benefits**:
- Automatic server discovery
- Better capability resolution
- Less manual configuration

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

### Option 6: Test & Validate Unified Service 🧪 (Quality)
**Priority**: High | **Effort**: 3-4 hours | **Impact**: High

**What**: Comprehensive testing of the unified MCP discovery service.

**Current State**:
- Basic example exists (`test_unified_mcp_discovery.rs`)
- But no comprehensive test suite

**Implementation**:
- Unit tests for `MCPDiscoveryService`
- Integration tests with mock MCP servers
- Test caching behavior
- Test error handling
- Test RTFS export/import

**Files to Create/Modify**:
- `ccos/tests/mcp_discovery_tests.rs` (new)
- `ccos/examples/test_unified_mcp_discovery.rs` (enhance)

**Benefits**:
- Confidence in unified service
- Catch regressions early
- Better documentation through tests

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

---

## 🎯 Suggested Order

1. **Option 6: Testing** (High priority, validates everything)
2. **Option 1: Output Schema Introspection** (Quick win, high impact)
3. **Option 3: Rate Limiting** (High priority, reliability)
4. **Option 2: Load RTFS on Startup** (Infrastructure improvement)
5. **Option 4: Registry Integration** (Discovery enhancement)
6. **Option 7: Performance** (Optimization)
7. **Option 5: Versioning** (Future maintenance)

---

## 💡 Quick Wins (Can Do Now)

- **Option 1**: Output schema introspection (code already exists, just needs wiring)
- **Option 6**: Add a few more test cases to existing example

---

## 🔮 Future Considerations

- **JIT Polyglot Generation**: See `docs/drafts/future-jit-polyglot-generation.md`
- **Discovery Hints & Re-planning**: See `docs/drafts/discovery_hints_replanning.md`
- **Schema-Adaptive Discovery**: See `docs/drafts/schema-adaptive-capability-discovery.md`

