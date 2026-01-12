# Unified MCP Discovery Service

**Status**: Authoritative  
**Version**: 1.0  
**Last Updated**: 2025-11-29  
**Scope**: Complete MCP discovery, caching, and capability mapping implementation

---

## 1. Overview

The **MCPDiscoveryService** is the single, unified API for discovering capabilities from Model Context Protocol (MCP) servers. It consolidates discovery logic from multiple modules and provides:

- Tool and resource discovery from MCP servers
- JSON Schema to RTFS TypeExpr conversion
- Automatic domain/category inference
- Caching with TTL and persistence
- Rate limiting and retry policies
- Parallel discovery with concurrency control
- Automatic marketplace and catalog registration

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                   MCPDiscoveryService                            │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  HTTP Client (shared, connection-pooled)                    ││
│  │  Session Manager (ephemeral sessions)                       ││
│  │  Rate Limiter (token bucket per server)                     ││
│  │  Cache (memory + file, configurable TTL)                    ││
│  │  Introspector (schema conversion + output inference)        ││
│  └─────────────────────────────────────────────────────────────┘│
│                              │                                   │
│         ┌────────────────────┼────────────────────┐             │
│         ▼                    ▼                    ▼             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │  Marketplace │    │   Catalog    │    │ RTFS Export  │      │
│  │  (optional)  │    │  (optional)  │    │  (optional)  │      │
│  └──────────────┘    └──────────────┘    └──────────────┘      │
└─────────────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐
│ Resolution       │  │  Marketplace     │  │   Introspector   │
│ Strategy (mcp.rs)│  │  (mcp_discovery) │  │  (synthesis)     │
└──────────────────┘  └──────────────────┘  └──────────────────┘
```

---

## 3. Core Components

### 3.1 MCPDiscoveryService

```rust
pub struct MCPDiscoveryService {
    // Shared HTTP client for connection pooling
    http_client: Arc<reqwest::Client>,
    
    // Session management for MCP protocol
    session_manager: Arc<MCPSessionManager>,
    
    // MCP Registry API client
    registry_client: MCPRegistryClient,
    
    // Local config-based server discovery
    config_discovery: LocalConfigMcpDiscovery,
    
    // Schema introspection and conversion
    introspector: MCPIntrospector,
    
    // Multi-layer cache (memory + file)
    cache: Arc<MCPCache>,
    
    // Rate limiting and retry
    rate_limiter: Arc<RateLimiter>,
    
    // Optional integrations
    marketplace: Option<Arc<CapabilityMarketplace>>,
    catalog: Option<Arc<CatalogService>>,
}
```

### 3.2 Key Methods

| Method | Purpose |
|--------|---------|
| `discover_tools(config, options)` | Discover tools from a single MCP server |
| `discover_resources(config, options)` | Discover resources from a server |
| `discover_from_registry(query, options)` | Search registry and discover from matching servers |
| `search_registry_for_capability(query, use_cache)` | Search MCP registry with caching |
| `find_servers_for_capability(name, options)` | Local-first search with registry fallback |
| `register_capability(manifest)` | Register in marketplace + catalog |
| `tool_to_manifest(tool, config)` | Convert discovered tool to capability manifest |
| `warm_cache_for_servers(servers, options)` | Pre-warm cache for servers |

---

## 4. Discovery Flow

### 4.1 Single Server Discovery

```
1. Check cache for server tools
   └─▶ Cache hit? Return cached tools
   
2. Rate limit check
   └─▶ Acquire token from bucket
   
3. Session initialization
   └─▶ HTTP POST /initialize → Mcp-Session-Id
   
4. Tool discovery
   └─▶ HTTP POST /tools/list with session
   
5. Schema conversion
   └─▶ JSON Schema → RTFS TypeExpr
   
6. Output schema introspection (optional)
   └─▶ Call tool with safe inputs to infer output type
   
7. Domain/category inference
   └─▶ Server name + tool name → domains, categories
   
8. Cache storage
   └─▶ Store tools with TTL
   
9. Registration (optional)
   └─▶ Marketplace + Catalog
   
10. Session termination
    └─▶ Clean up session state
```

### 4.2 Registry-Based Discovery

```rust
// Search registry, discover from matching servers in parallel
let results = discovery_service.discover_from_registry("github", &options).await?;

// Results: Vec<(MCPServerConfig, Vec<DiscoveredMCPTool>)>
for (server_config, tools) in results {
    println!("Server: {} → {} tools", server_config.name, tools.len());
}
```

Flow:
1. Query MCP Registry API for servers matching query
2. Cache registry results (1-hour TTL)
3. Discover from each server in parallel (semaphore-limited)
4. Aggregate results with error handling per server

---

## 5. Schema Conversion

### 5.1 JSON Schema to RTFS TypeExpr

| JSON Schema | RTFS TypeExpr |
|-------------|---------------|
| `"string"` | `:string` |
| `"integer"`, `"number"` | `:int`, `:float` |
| `"boolean"` | `:bool` |
| `"array"` | `[:vector <item-type>]` |
| `"object"` | `[:map ...]` |
| `"null"` or nullable | `:any` (with context) |
| `enum: [...]` | `:string` (with validation) |

### 5.2 Example Conversion

**JSON Schema (from MCP server):**
```json
{
  "type": "object",
  "properties": {
    "owner": { "type": "string" },
    "repo": { "type": "string" },
    "state": { "type": "string", "enum": ["open", "closed", "all"] }
  },
  "required": ["owner", "repo"]
}
```

**RTFS TypeExpr:**
```clojure
[:map
  [:owner :string]
  [:repo :string]
  [:state {:optional true} :string]]
```

### 5.3 Output Schema Introspection

When MCP servers don't provide output schemas (common), the system can infer them:

1. Generate safe test inputs based on input schema
2. Call the tool once during discovery
3. Parse the response and infer types from JSON values
4. Store inferred schema in manifest

**Configuration:**
```rust
let options = DiscoveryOptions {
    introspect_output_schemas: true,  // Enable inference
    lazy_output_schemas: false,       // Force immediate introspection
    ..Default::default()
};
```

---

## 6. Domain and Category Inference

When tools are discovered, domains and categories are automatically inferred:

### 6.1 Domain Extraction

```rust
// From server name
CapabilityManifest::extract_primary_domain("modelcontextprotocol/github")
// → "github"

CapabilityManifest::extract_primary_domain("aws-s3-server")
// → "cloud.aws"

// From tool name (sub-domain)
CapabilityManifest::extract_sub_domain("list_issues")
// → "issues"

CapabilityManifest::extract_sub_domain("get_pull_request")
// → "pull_requests"

// Combined
CapabilityManifest::infer_domains("github", "list_issues")
// → ["github", "github.issues"]
```

### 6.2 Category Extraction

```rust
CapabilityManifest::infer_category("list_issues")
// → ["crud.read"]

CapabilityManifest::infer_category("create_issue")
// → ["crud.create"]

CapabilityManifest::infer_category("search_code")
// → ["crud.read", "search"]
```

### 6.3 Integration Point

```rust
// In tool_to_manifest()
let mut manifest = CapabilityManifest::new(...);
manifest = manifest.with_inferred_domains_and_categories(&server_config.name);
```

---

## 7. Caching Layer

### 7.1 Cache Structure

```rust
pub struct MCPCache {
    // In-memory cache
    memory_cache: RwLock<HashMap<String, CachedTools>>,
    
    // File-based persistence
    cache_dir: PathBuf,
    
    // TTL configuration
    tool_cache_ttl: Duration,      // Default: 24 hours
    registry_cache_ttl: Duration,  // Default: 1 hour
}
```

### 7.2 Cache Keys

- Tool cache: `tools_<sanitized_server_name>`
- Registry cache: `registry_<sanitized_query>`

### 7.3 Cache Operations

```rust
// Check cache
let cached = cache.get_tools(&server_config.name);

// Store with TTL
cache.store_tools(&server_config.name, &tools);

// Clear all caches
cache.clear();

// Clear specific server
cache.clear_server(&server_config.name);
```

---

## 8. Rate Limiting

### 8.1 Token Bucket Algorithm

```rust
pub struct RateLimitConfig {
    pub requests_per_second: f64,  // Default: 10.0
    pub burst_size: u32,           // Default: 20
}

pub struct RateLimiter {
    default_config: RateLimitConfig,
    per_server_configs: HashMap<String, RateLimitConfig>,
    buckets: RwLock<HashMap<String, TokenBucket>>,
}
```

### 8.2 Retry Policy

```rust
pub struct RetryPolicy {
    pub max_retries: u32,           // Default: 3
    pub initial_delay_ms: u64,      // Default: 1000
    pub max_delay_ms: u64,          // Default: 30000
    pub backoff_multiplier: f64,    // Default: 2.0
    pub jitter_factor: f64,         // Default: 0.1
}
```

### 8.3 Retryable Errors

- HTTP 429 (Too Many Requests)
- HTTP 503 (Service Unavailable)
- Timeout errors
- Connection reset errors

---

## 9. Parallel Discovery

### 9.1 Concurrency Control

```rust
let options = DiscoveryOptions {
    max_parallel_discoveries: 5,  // Limit concurrent server discoveries
    ..Default::default()
};

// Uses tokio::sync::Semaphore internally
let results = discovery_service.discover_from_registry("github", &options).await?;
```

### 9.2 Performance

| Scenario | Sequential | Parallel (5) | Improvement |
|----------|------------|--------------|-------------|
| 10 servers × 2s | 20s | ~3s | 85% faster |
| 5 servers × 1s | 5s | ~1.5s | 70% faster |

---

## 10. Cache Warming

### 10.1 On-Demand Warming

```rust
let servers = vec![server1, server2, server3];
let options = DiscoveryOptions {
    use_cache: true,
    lazy_output_schemas: true,  // Skip expensive introspection
    ..Default::default()
};

let stats = discovery_service.warm_cache_for_servers(&servers, &options).await?;
println!("Warmed: {} successful, {} failed, {} tools cached", 
    stats.successful, stats.failed, stats.cached_tools);
```

### 10.2 Startup Warming

```rust
// Warm all configured servers
let stats = discovery_service.warm_cache_for_all_configured_servers(&options).await?;
```

---

## 11. RTFS Export

Discovered tools can be exported as RTFS capability definitions:

### 11.1 Export Options

```rust
let options = DiscoveryOptions {
    export_to_rtfs: true,
    export_directory: Some("capabilities/servers/pending".to_string()),
    ..Default::default()
};
```

### 11.2 Generated File Structure

```
capabilities/servers/pending/
└── github/
    ├── list_issues.rtfs
    ├── create_issue.rtfs
    └── search_code.rtfs
```

### 11.3 Approval and Promotion

Discovered capabilities are stored in the `pending` directory and are **not** loaded by the marketplace by default. To activate them, they must go through the approval process:

1. **Introspection**: `ccos_introspect_remote_api` discovers tools and creates an approval request.
2. **Review**: The user reviews the generated RTFS files in `pending/`.
3. **Approval**: Upon approval, the `ccos_register_server` tool is invoked.
4. **Promotion**: The service moves the RTFS files from `capabilities/servers/pending/` to `capabilities/servers/approved/`.
5. **Registration**: The promoted capabilities are registered in the marketplace.

### 11.4 RTFS Format

```clojure
(capability :mcp.github.list_issues
  :version "1.0.0"
  :description "List issues in a GitHub repository"
  :input-schema [:map
    [:owner :string]
    [:repo :string]
    [:state {:optional true} :string]]
  :output-schema [:vector [:map
    [:id :int]
    [:title :string]
    [:state :string]]]
  :provider {:type :mcp
             :server-url "https://api.github.com/mcp"
             :tool-name "list_issues"}
  :domains ["github" "github.issues"]
  :categories ["crud.read"])
```

---

## 12. Integration with Callers

### 12.1 Resolution Strategy (mcp.rs)

```rust
pub struct RuntimeMcpDiscovery {
    discovery_service: Arc<MCPDiscoveryService>,
    marketplace: Option<Arc<CapabilityMarketplace>>,
}

impl RuntimeMcpDiscovery {
    pub fn new(marketplace: Option<Arc<CapabilityMarketplace>>) -> Self {
        Self {
            discovery_service: Arc::new(MCPDiscoveryService::new()),
            marketplace,
        }
    }
    
    pub fn with_discovery_service(
        discovery_service: Arc<MCPDiscoveryService>,
        marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Self { ... }
}
```

### 12.2 Marketplace Discovery (mcp_discovery.rs)

```rust
pub struct MCPDiscoveryProvider {
    discovery_service: Arc<MCPDiscoveryService>,
    config: MCPServerConfig,
}

impl MCPDiscoveryProvider {
    pub fn new(config: MCPServerConfig) -> Self {
        let auth_headers = build_auth_headers(&config);
        Self {
            discovery_service: Arc::new(
                MCPDiscoveryService::with_auth_headers(auth_headers)
            ),
            config,
        }
    }
}
```

---

## 13. Configuration Reference

### 13.1 Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MCP_AUTH_TOKEN` | - | Generic bearer token for MCP servers |
| `GITHUB_MCP_TOKEN` | - | GitHub-specific MCP token |
| `CCOS_MCP_CACHE_DIR` | `.ccos/mcp_cache` | Cache directory |
| `CCOS_MCP_CACHE_TTL` | `86400` | Tool cache TTL in seconds |

### 13.2 Discovery Options

```rust
DiscoveryOptions {
    introspect_output_schemas: false,  // Expensive, disabled by default
    use_cache: false,                   // Must be explicitly enabled
    register_in_marketplace: false,     // Auto-register discovered tools
    export_to_rtfs: false,              // Persist to files
    export_directory: None,             // Default: capabilities/servers/pending
    auth_headers: None,                 // Override server auth
    retry_policy: RetryPolicy::default(),
    rate_limit: RateLimitConfig::default(),
    max_parallel_discoveries: 5,        // Concurrency limit
    lazy_output_schemas: true,          // Skip introspection by default
}
```

---

## 14. Error Handling

### 14.1 Discovery Errors

```rust
pub enum MCPDiscoveryError {
    ConnectionFailed(String),
    SessionInitFailed(String),
    ToolsListFailed(String),
    SchemaConversionFailed(String),
    RateLimited(Duration),  // Retry after
    AuthenticationFailed,
    Timeout,
}
```

### 14.2 Graceful Degradation

- Server unreachable → Skip, continue with others
- Rate limited → Exponential backoff retry
- Schema conversion fails → Use `:any` type
- Output introspection fails → Leave output_schema as None

---

## 15. Testing

### 15.1 Test Coverage

- 26+ tests in `ccos/tests/mcp_discovery_tests.rs`
- Cache tests (memory, file, TTL, clear)
- Discovery options tests
- Schema conversion tests
- Rate limiting tests
- Integration tests

### 15.2 Running Tests

```bash
cd ccos
cargo test --test mcp_discovery_tests
cargo test --lib mcp
```

---

## 16. File Locations

| Component | Location |
|-----------|----------|
| Core Service | `ccos/src/mcp/core.rs` |
| Session Manager | `ccos/src/mcp/discovery_session.rs` |
| Registry Client | `ccos/src/mcp/registry.rs` |
| Cache | `ccos/src/mcp/cache.rs` |
| Rate Limiter | `ccos/src/mcp/rate_limiter.rs` |
| Types | `ccos/src/mcp/types.rs` |
| Module | `ccos/src/mcp/mod.rs` |
| Tests | `ccos/tests/mcp_discovery_tests.rs` |

---

## 17. See Also

- [030-capability-system-architecture.md](./030-capability-system-architecture.md) - Overall capability system
- [032-missing-capability-resolution.md](./032-missing-capability-resolution.md) - Resolution when capabilities are missing
