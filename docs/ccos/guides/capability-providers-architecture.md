# Capability Providers Architecture

## Current State (After API Introspection Feature)

### ✅ What Works Well

1. **API Introspection** (`api_introspector.rs`)
   - Discovers endpoints from OpenAPI specs
   - Converts JSON Schema → RTFS TypeExpr
   - Generates proper input/output schemas
   - Creates one CapabilityManifest per endpoint

2. **RTFS-Based Execution**
   - Generated `.rtfs` files with `:implementation` field
   - Clean, runtime-controlled code (~30 lines)
   - API key injection via `ccos.system.get-env`
   - Works with existing RTFS evaluator

3. **Schema Serialization** (`schema_serializer.rs`)
   - Shared utility for TypeExpr → RTFS conversion
   - Supports both compact and pretty formatting
   - Used by multiple synthesis modules

## 🏗️ Provider Architecture

### Provider Types

```rust
pub enum ProviderType {
    Local(LocalCapability),      // Rust function handler
    Http(HttpCapability),         // HTTP API (minimal - only base_url)
    MCP(MCPCapability),           // MCP server tool
    A2A(A2ACapability),           // Agent-to-agent
    Plugin(PluginCapability),     // Plugin system
    RemoteRTFS(RemoteRTFSCapability), // Remote RTFS execution
    Stream(StreamCapabilityImpl), // Streaming capabilities
    Registry(RegistryCapability), // Registry lookup
}
```

### Current Usage for Synthesized Capabilities

**OpenAPI/REST APIs:**
```rust
provider: ProviderType::Local(LocalCapability {
    handler: Arc::new(|_| Ok(Value::String("placeholder")))
})
```
- ⚠️ Handler is a placeholder
- ✅ Actual implementation in `.rtfs` file
- ✅ Execution via RTFS evaluator (not marketplace executor)

**MCP Servers:**
```rust
provider: ProviderType::MCP(MCPCapability {
    server_url: "http://localhost:3000",
    tool_name: "get_weather",
    timeout_ms: 30000,
})
```
- ✅ Handler connects to real MCP server
- ✅ Execution via MCPExecutor
- ✅ No RTFS needed (direct Rust→MCP)

## 🔄 Execution Paths

### Path 1: Marketplace Provider Execution
```
(capability registered with ProviderType::MCP)
User: (call "mcp.weather.get_current")
     ↓
Marketplace.execute_capability()
     ↓
MCPExecutor.execute()
     ↓
HTTP request to MCP server
     ↓
Parse MCP response
     ↓
Return Value
```

### Path 2: RTFS Function Execution (Current for Synthesized)
```
(capability.rtfs loaded with :implementation)
User: ((call "openweather_api.get_current_weather") {...})
     ↓
RTFS Evaluator
     ↓
Lookup symbol "openweather_api.get_current_weather"
     ↓
Execute RTFS function from :implementation
     ↓
Function calls ccos.network.http-fetch
     ↓
HTTP request made
     ↓
Return Value
```

## 🤔 Do We Need providers/weather_mcp.rs and providers/github_mcp.rs?

### Analysis

**`weather_mcp.rs`:**
- 535 lines of hardcoded Weather MCP provider
- Implements `CapabilityProvider` trait
- Has hardcoded tool definitions
- **Verdict:** ❌ REDUNDANT with API introspection
  - Can be replaced by introspecting OpenWeather API
  - Or connecting to a real MCP weather server

**`github_mcp.rs`:**
- 775 lines of hardcoded GitHub MCP provider
- Implements `CapabilityProvider` trait  
- Has hardcoded tool definitions
- **Verdict:** ❌ REDUNDANT with MCP introspection
  - Should use real GitHub MCP server instead
  - Or synthesize from GitHub OpenAPI spec

### Recommendation

1. **Move to examples/** - Keep as reference implementations
2. **Deprecate** - Mark as deprecated in favor of MCP discovery
3. **Or Delete** - If MCP discovery works well

## 🎯 Clean Architecture for Synthesized Capabilities

### Current Approach: RTFS-First ✅

**Pros:**
- ✅ Works today without new infrastructure
- ✅ RTFS provides flexibility and composability
- ✅ Easy to modify/debug (just edit .rtfs file)
- ✅ Schemas properly encoded for validation
- ✅ Can use all RTFS stdlib functions

**Cons:**
- ⚠️ Placeholder provider in marketplace
- ⚠️ Two separate registration steps (manifest + rtfs file)
- ⚠️ Performance overhead of RTFS evaluation

### Future Approach: Provider-First

**Option 1: Enhanced HttpCapability**
```rust
pub struct HttpCapability {
    pub base_url: String,
    pub endpoint: String,           // NEW
    pub method: String,              // NEW
    pub auth_config: Option<AuthConfig>,  // NEW
    pub headers: HashMap<String, String>, // NEW
    pub query_params: Vec<QueryParam>,    // NEW
    pub timeout_ms: u64,
}
```

**Option 2: RTFS Provider**
```rust
pub struct RTFSCapability {
    pub implementation_code: String,  // The RTFS code
    pub env: Arc<RwLock<Environment>>, // Execution environment
}
```

Then synthesized capabilities could be:
```rust
provider: ProviderType::Local(RTFSCapability {
    implementation_code: rtfs_impl,
    env: shared_env,
})
```

## 📋 Refactoring Roadmap

### Phase 1: Current State (✅ Done)
- ✅ API introspection working
- ✅ Schemas properly encoded
- ✅ RTFS implementations clean
- ✅ Multi-capability generation

### Phase 2: Clean Up Redundancy
- [ ] Move `providers/weather_mcp.rs` to `examples/`
- [ ] Move `providers/github_mcp.rs` to `examples/`
- [ ] Update docs to clarify their status
- [ ] Add deprecation notices

### Phase 3: Enhance HttpCapability (Optional)
- [ ] Add `endpoint`, `method`, `query_params` fields
- [ ] Implement HttpExecutor that builds URLs, adds auth, makes requests
- [ ] Update `api_introspector` to use `ProviderType::Http`
- [ ] Remove RTFS implementation from synthesized HTTP APIs

### Phase 4: Unify Execution (Optional)
- [ ] All capabilities execute via marketplace
- [ ] RTFS capabilities use `RTFSCapability` provider
- [ ] Consistent audit logging, rate limiting, security checks

## 🎯 Current Recommendation: Keep As-Is

### Why?

1. **It works!** The RTFS-first approach is functional
2. **Clean separation:** Marketplace = metadata/schemas, RTFS = execution
3. **No breaking changes:** Existing capabilities continue to work
4. **Future-proof:** Can enhance HttpCapability later without breaking synthesis

### What to Do Now

1. ✅ **Keep** current RTFS-first approach
2. ✅ **Document** the execution model clearly
3. ✅ **Deprecate** manual providers (weather_mcp, github_mcp)
4. ⏭️ **Later:** Enhance HttpCapability when needed

## 📄 Summary

**Current State:**
- Synthesized capabilities use `ProviderType::Local` with placeholder handler
- Actual execution happens via RTFS function from `.rtfs` file
- Works well, clean, maintainable

**Manual Providers (`providers/`):**
- `weather_mcp.rs` and `github_mcp.rs` are hardcoded examples
- They implement `CapabilityProvider` trait
- They are **NOT used** by synthesized capabilities
- **Recommendation:** Deprecate or move to examples

**No changes needed right now** - the architecture is clean enough for synthesized capabilities!

