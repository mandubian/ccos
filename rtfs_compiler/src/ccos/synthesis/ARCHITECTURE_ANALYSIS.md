# Capability Provider Architecture Analysis

## 🎯 Key Finding: Two Separate Execution Paths

There are **two different ways** capabilities are executed in CCOS:

###  1. Marketplace Provider Execution (Runtime Providers)
```
User calls capability
     ↓
Marketplace.execute_capability()
     ↓
Get CapabilityManifest
     ↓
Route to executor based on ProviderType
     ↓
ProviderType::MCP → MCPExecutor → HTTP call to MCP server
ProviderType::Http → HttpExecutor → HTTP call
ProviderType::Local → LocalExecutor → Call handler function
```

### 2. RTFS Function Execution (Synthesized Capabilities)
```
User calls capability
     ↓
RTFS evaluator: (call "capability-name")
     ↓
Lookup symbol in environment
     ↓
Execute RTFS function from :implementation field
     ↓
Function makes calls to ccos.network.http-fetch, etc.
```

## 🔴 The Problem

### Current State of Synthesized Capabilities

**In `CapabilityManifest`:**
```rust
provider: ProviderType::Local(LocalCapability {
    handler: Arc::new(|_| {
        Ok(Value::String("Placeholder".to_string()))  // ← NEVER USED!
    }),
})
```

**In `capability.rtfs` file:**
```clojure
:implementation
  (fn [input]
    (let [...]
      (call "ccos.network.http-fetch" ...)))  // ← ACTUALLY USED!
```

### The Disconnect

1. The **CapabilityManifest** is registered in marketplace with a **placeholder** handler
2. The **RTFS implementation** is loaded as a separate function in the environment
3. Calling `(call "name")` bypasses the marketplace entirely!
4. The marketplace provider system is **not being used** for synthesized capabilities

## ✅ What SHOULD Happen

### Option A: Use Marketplace Execution (Recommended)

Synthesized capabilities should use a proper provider that executes the RTFS code:

```rust
provider: ProviderType::Local(LocalCapability {
    handler: Arc::new(move |inputs| {
        // Parse and execute the RTFS implementation code
        env.execute_code(&rtfs_implementation_code)?
    }),
})
```

### Option B: Keep RTFS-Only (Current Approach)

Accept that synthesized capabilities are **RTFS-first** and the marketplace is just metadata:
- ✅ Marketplace stores schemas for validation
- ✅ Marketplace stores metadata for discovery
- ❌ Marketplace provider is just a placeholder
- ⚠️ Actual execution happens via RTFS environment

## 🔍 Analysis of providers/

### `weather_mcp.rs`
- **Purpose:** Hardcoded MCP provider for Weather API
- **When Used:** When registered via `ProviderType::MCP`
- **Relation to Synthesis:** NONE - this is a manual provider
- **Status:** Legacy? Replaced by synthesized capabilities?

### `github_mcp.rs`
- **Purpose:** Hardcoded MCP provider for GitHub API
- **When Used:** When registered via `ProviderType::MCP`
- **Relation to Synthesis:** NONE - this is a manual provider
- **Status:** Could be replaced by MCP introspection

### Do We Need These?

**NO for synthesized capabilities!** Our introspected capabilities:
1. Generate RTFS code directly
2. Execute via RTFS evaluator
3. Don't use the CapabilityProvider trait
4. Don't need MCPExecutor/HttpExecutor

**YES for runtime-native capabilities!** If you want:
1. Capabilities that don't require RTFS parsing
2. Direct Rust → API integration
3. Better performance (no RTFS overhead)
4. Streaming capabilities (like MCP servers)

## 🎯 Recommendation: Clean Architecture

### For OpenAPI/Swagger Introspection

```rust
// Option 1: Pure RTFS (current approach) ✅
provider: ProviderType::Local(LocalCapability {
    handler: Arc::new(rtfs_wrapper_that_executes_implementation)
})

// Option 2: Http Provider (cleaner)
provider: ProviderType::Http(HttpCapability {
    base_url: "https://api.openweathermap.org",
    endpoint: "/data/2.5/weather",
    method: "GET",
    auth_config: AuthConfig { ... }
})
```

### For MCP Introspection

```rust
// Use MCP provider (already correct!)
provider: ProviderType::MCP(MCPCapability {
    server_url: "http://localhost:3000",
    tool_name: "get_weather",
    timeout_ms: 30000,
})
```

## 📋 TODO: Clean Up & Consolidate

### 1. ✅ Already Good
- `api_introspector.rs` - Clean introspection logic
- Schema conversion - Using shared `schema_serializer`
- Multiple capabilities per API

### 2. 🔧 Needs Improvement

**Create Proper Providers for Synthesized Capabilities:**

```rust
// In api_introspector.rs - Instead of placeholder:
provider: ProviderType::Http(HttpCapability {
    base_url: introspection.base_url.clone(),
    endpoint: endpoint.path.clone(),
    method: endpoint.method.clone(),
    auth_config: Some(AuthConfig {
        auth_type: AuthType::ApiKey,
        key_location: AuthLocation::Query("appid"),
        env_var: Some("OPENWEATHERMAP_ORG_API_KEY"),
    }),
    headers: HashMap::new(),
    query_params: endpoint.parameters.clone(),
})
```

### 3. ❓ Question Marks

**Do we keep `providers/weather_mcp.rs` and `providers/github_mcp.rs`?**
- If we have MCP introspection → NO, they're redundant
- If we want hardcoded examples → YES, but move to examples/
- If they have special logic → Extract to runtime utilities

## 🚀 Proposed Refactoring

###  1. Create `HttpCapability` with proper execution
### 2. Update `api_introspector` to use `ProviderType::Http` instead of `Local` placeholder
### 3. Remove or deprecate manual `providers/weather_mcp.rs` (redundant with introspection)
### 4. Keep MCP execution infrastructure for real MCP server connections
### 5. Document the two execution paths clearly

## 📊 Summary

| Capability Type | Provider | Execution Path | Current Status |
|-----------------|----------|----------------|----------------|
| **Synthesized OpenAPI** | ProviderType::Local (placeholder) | RTFS evaluator | ⚠️ Disconnect |
| **MCP Introspected** | ProviderType::MCP | MCPExecutor → MCP server | ✅ Correct |
| **Manual weather_mcp** | ProviderType::MCP | MCPExecutor | ❓ Redundant? |
| **Manual github_mcp** | ProviderType::MCP | MCPExecutor | ❓ Redundant? |

**Recommendation:** Use `ProviderType::Http` for synthesized HTTP APIs instead of `Local` with RTFS implementation.

