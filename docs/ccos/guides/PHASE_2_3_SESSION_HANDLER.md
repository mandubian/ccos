# Phase 2.3: Session Handler Delegation

## Goal

Implement **generic session management** that:
1. Reads capability metadata hints
2. Delegates to provider-specific session handlers
3. Manages session pools (initialization, reuse, cleanup)
4. Remains completely provider-agnostic in the registry

## Architecture

### High-Level Flow

```
Capability Execution Request
         │
         ▼
┌─────────────────────────────────┐
│ Registry: Check Metadata        │ ← Phase 2.2 (Done)
│ metadata.get("X_requires_session") │
└────────────┬────────────────────┘
             │
             ▼ (if session required)
┌─────────────────────────────────┐
│ SessionPoolManager (Generic)    │ ← Phase 2.3 (This)
│ - Get or create session         │
│ - Delegate to provider handler  │
└────────────┬────────────────────┘
             │
        ┌────┴────┐
        │         │
        ▼         ▼
┌───────────┐ ┌────────────┐
│ MCPSession│ │ GraphQLSess│  ... future providers
│ Handler   │ │ Handler    │
└───────────┘ └────────────┘
```

### Components

#### 1. SessionPoolManager (Generic)
**Location**: `rtfs_compiler/src/ccos/capabilities/session_pool.rs` (new file)

**Responsibilities**:
- Registry for session handlers (by provider type)
- Routes session requests to appropriate handler
- Provides generic session lifecycle hooks
- **Zero provider-specific logic**

**Interface**:
```rust
pub trait SessionHandler: Send + Sync {
    /// Initialize a new session for a capability
    fn initialize_session(&self, metadata: &HashMap<String, String>) -> RuntimeResult<SessionId>;
    
    /// Execute capability call with existing session
    fn execute_with_session(
        &self,
        session_id: &SessionId,
        capability_id: &str,
        args: Vec<Value>,
    ) -> RuntimeResult<Value>;
    
    /// Terminate a session (cleanup)
    fn terminate_session(&self, session_id: &SessionId) -> RuntimeResult<()>;
}

pub struct SessionPoolManager {
    handlers: HashMap<String, Box<dyn SessionHandler>>,
}

impl SessionPoolManager {
    pub fn new() -> Self { ... }
    
    /// Register a handler for a provider type (e.g., "mcp", "graphql")
    pub fn register_handler(&mut self, provider_type: &str, handler: Box<dyn SessionHandler>) { ... }
    
    /// Execute capability with session management (generic)
    pub fn execute_with_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
        args: Vec<Value>,
    ) -> RuntimeResult<Value> {
        // 1. Determine provider from metadata (e.g., "mcp_*" keys → "mcp")
        // 2. Get or create session
        // 3. Delegate to handler.execute_with_session()
        // 4. Handle errors (retry, session refresh, etc.)
    }
}
```

#### 2. MCPSessionHandler (Provider-Specific)
**Location**: `rtfs_compiler/src/ccos/capabilities/mcp_session_handler.rs` (new file)

**Responsibilities**:
- MCP-specific session lifecycle (initialize, tools/call, terminate)
- Session pool management (reuse, limits)
- Auth token injection from env vars
- Error handling and session refresh

**Implementation**:
```rust
pub struct MCPSessionHandler {
    sessions: Arc<Mutex<HashMap<String, MCPSession>>>, // session pool
    http_client: Arc<reqwest::Client>,
}

struct MCPSession {
    session_id: String,
    server_url: String,
    auth_token: Option<String>,
    created_at: std::time::Instant,
}

impl SessionHandler for MCPSessionHandler {
    fn initialize_session(&self, metadata: &HashMap<String, String>) -> RuntimeResult<SessionId> {
        // 1. Extract server_url from metadata
        // 2. Get auth token from metadata["mcp_auth_env_var"]
        // 3. Call MCP initialize endpoint
        // 4. Store session in pool
        // 5. Return SessionId
    }
    
    fn execute_with_session(...) -> RuntimeResult<Value> {
        // 1. Get session from pool
        // 2. Build MCP JSON-RPC request
        // 3. Add Mcp-Session-Id header
        // 4. Make HTTP call
        // 5. Parse response
        // 6. Return result
    }
    
    fn terminate_session(...) -> RuntimeResult<()> {
        // 1. Get session from pool
        // 2. Call MCP terminate endpoint
        // 3. Remove from pool
    }
}
```

#### 3. Registry Integration
**Location**: `rtfs_compiler/src/ccos/capabilities/registry.rs` (modify)

**Changes**:
```rust
pub struct CapabilityRegistry {
    // ... existing fields ...
    session_pool: Option<Arc<SessionPoolManager>>, // ← NEW
}

impl CapabilityRegistry {
    pub fn set_session_pool(&mut self, pool: Arc<SessionPoolManager>) {
        self.session_pool = Some(pool);
    }
    
    pub fn execute_capability_with_microvm(...) -> RuntimeResult<Value> {
        // ... security checks ...
        
        // GENERIC METADATA-DRIVEN ROUTING (Phase 2.2)
        if let Some(metadata) = self.get_capability_metadata(capability_id) {
            // Check ANY provider's session requirements
            if self.requires_session(&metadata) { // ← NEW helper
                if let Some(pool) = &self.session_pool {
                    // Delegate to session pool (completely generic!)
                    return pool.execute_with_session(capability_id, &metadata, args);
                }
            }
        }
        
        // ... normal execution ...
    }
    
    /// Generic helper: checks if ANY provider requires session
    fn requires_session(&self, metadata: &HashMap<String, String>) -> bool {
        // Check for *_requires_session keys (any provider)
        metadata.iter().any(|(k, v)| {
            k.ends_with("_requires_session") && (v == "true" || v == "auto")
        })
    }
}
```

## Implementation Plan

### Step 1: Create SessionPoolManager (Generic)
- Define `SessionHandler` trait
- Implement `SessionPoolManager` with handler registry
- Add `execute_with_session()` with generic routing logic

### Step 2: Implement MCPSessionHandler
- Reuse existing `MCPSessionManager` from Phase 1
- Adapt to `SessionHandler` trait
- Add session pooling and reuse logic
- Handle auth token from metadata

### Step 3: Integrate into Registry
- Add `session_pool` field to `CapabilityRegistry`
- Update `execute_capability_with_microvm()` to delegate
- Add `requires_session()` helper

### Step 4: Wire Up in Environment
- Create `SessionPoolManager` in `CCOSEnvironment::new()`
- Register `MCPSessionHandler`
- Set pool in registry via `set_session_pool()`

### Step 5: Test End-to-End
- Update `test_metadata_routing.rs` to use sessions
- Verify MCP capabilities work with auth
- Test session reuse and pooling
- Verify error handling (session expired, etc.)

## Metadata-Driven Behavior

### MCP Capability Metadata
```rtfs
:metadata {
  :mcp {
    :server_url "https://api.githubcopilot.com/mcp/"
    :requires_session "auto"
    :auth_env_var "MCP_AUTH_TOKEN"
  }
}
```

**Runtime Behavior**:
1. Registry sees `mcp_requires_session = "auto"`
2. Delegates to `SessionPoolManager`
3. Manager routes to `MCPSessionHandler` (based on `mcp_*` keys)
4. Handler initializes session with auth from `MCP_AUTH_TOKEN`
5. Handler executes with `Mcp-Session-Id` header
6. Session is reused for subsequent calls

### Future: GraphQL Capability Metadata
```rtfs
:metadata {
  :graphql {
    :endpoint "https://api.github.com/graphql"
    :requires_session "true"
    :auth_env_var "GITHUB_GRAPHQL_TOKEN"
    :pool_size "10"
  }
}
```

**Runtime Behavior** (no code changes needed):
1. Registry sees `graphql_requires_session = "true"`
2. Delegates to `SessionPoolManager`
3. Manager routes to `GraphQLSessionHandler` (when registered)
4. Same generic flow, different handler

## Key Design Principles

### 1. Provider-Agnostic Registry
```rust
// ❌ Bad (provider-specific)
if metadata.get("mcp_requires_session") == Some(&"auto") {
    handle_mcp_session();
}

// ✅ Good (generic)
if self.requires_session(&metadata) {
    self.session_pool.execute_with_session(...);
}
```

### 2. Metadata as Contract
Capabilities declare: "I need sessions"
```rtfs
:mcp { :requires_session "auto" }
```

Runtime provides: "Here's your session"
```rust
session_pool.execute_with_session(capability_id, metadata, args)
```

### 3. Handler Isolation
Each provider implements `SessionHandler` independently:
- MCP handler knows MCP protocol
- GraphQL handler knows GraphQL protocol
- Registry knows NOTHING about either

### 4. Extensibility
Adding a new stateful provider:
1. Implement `SessionHandler` for the provider
2. Register handler: `pool.register_handler("newprovider", ...)`
3. Capabilities use `:newprovider { :requires_session "true" }`
4. Zero changes to registry!

## Testing Strategy

### Unit Tests
1. `SessionPoolManager::register_handler()` and routing
2. `MCPSessionHandler` session lifecycle
3. `requires_session()` helper with various metadata

### Integration Tests
1. Load MCP capability with `requires_session`
2. Call capability (should initialize session)
3. Call again (should reuse session)
4. Verify `Mcp-Session-Id` header in requests
5. Test auth token injection

### End-to-End Test
Update `test_metadata_routing.rs`:
```rust
// Setup session pool
let mut pool = SessionPoolManager::new();
pool.register_handler("mcp", Box::new(MCPSessionHandler::new()));
env.registry().set_session_pool(Arc::new(pool));

// Load MCP capability
env.execute_file("capabilities/mcp/github/get_me.rtfs")?;

// Set auth token
std::env::set_var("MCP_AUTH_TOKEN", "test_token");

// Call capability (should work with session!)
let result = env.execute_code(r#"
    ((call "mcp.github.get_me") {})
"#)?;

// Verify success (not 401 anymore!)
assert!(!format!("{:?}", result).contains("401"));
```

## Success Criteria

✅ **Generic session management**: No MCP-specific code in registry  
✅ **Metadata-driven**: Session handling based on capability metadata  
✅ **Session pooling**: Reuses sessions across calls  
✅ **Auth injection**: Reads tokens from env vars via metadata  
✅ **Error handling**: Handles session expiry, refresh, retries  
✅ **Extensible**: Easy to add GraphQL, gRPC, custom session handlers  
✅ **Tested**: MCP capabilities work with real GitHub API  

## Files to Create/Modify

### New Files
1. `rtfs_compiler/src/ccos/capabilities/session_pool.rs`
   - `SessionHandler` trait
   - `SessionPoolManager` struct

2. `rtfs_compiler/src/ccos/capabilities/mcp_session_handler.rs`
   - `MCPSessionHandler` implementing `SessionHandler`
   - Session pool management

### Modified Files
1. `rtfs_compiler/src/ccos/capabilities/registry.rs`
   - Add `session_pool` field
   - Add `requires_session()` helper
   - Update `execute_capability_with_microvm()`

2. `rtfs_compiler/src/ccos/capabilities/mod.rs`
   - Add `pub mod session_pool;`
   - Add `pub mod mcp_session_handler;`

3. `rtfs_compiler/src/ccos/environment.rs`
   - Create and wire up `SessionPoolManager`
   - Register `MCPSessionHandler`

4. `rtfs_compiler/src/bin/test_metadata_routing.rs`
   - Test session management end-to-end

## Timeline

**Estimated**: 2-3 hours
- 1 hour: Generic session pool infrastructure
- 1 hour: MCP session handler
- 30 min: Registry integration
- 30 min: Testing and refinement

## Dependencies

- ✅ Phase 2.1: Metadata parsing (complete)
- ✅ Phase 2.2: Registry integration (complete)
- ✅ Existing `MCPSessionManager` from Phase 1 (can reuse)

## Risks & Mitigation

### Risk: Session Pooling Complexity
**Mitigation**: Start with simple 1:1 mapping (capability_id → session), optimize later

### Risk: Auth Token Security
**Mitigation**: Always read from env vars, never log tokens

### Risk: Session Expiry Handling
**Mitigation**: Add TTL tracking, auto-refresh on 401 errors

## Next Steps After Phase 2.3

With session management complete:
1. **Phase 3**: Implement GraphQL session handler (demonstrates extensibility)
2. **Phase 4**: Add rate limiting metadata hints
3. **Phase 5**: Implement capability attestation and provenance
4. **Phase 6**: Full production deployment

## Conclusion

Phase 2.3 completes the metadata-driven architecture by implementing the actual session management that metadata hints enable. The key is maintaining perfect provider-agnosticism: the registry delegates generically, handlers implement specifically.

This design scales to unlimited provider types and session patterns without polluting the core execution logic.

