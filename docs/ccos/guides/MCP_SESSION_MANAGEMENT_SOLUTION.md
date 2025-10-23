# MCP Session Management Solution

## Problem Statement

MCP capabilities need a way to indicate when they require session management (like GitHub Copilot MCP API that requires `initialize` → `tools/call` → `terminate` flow with session IDs).

**Initial approach was problematic:**
- Hardcoded `MCP_USE_SESSION_MGMT` environment variable
- GitHub-specific logic (`GITHUB_PAT`) in generic MCP capabilities
- Mixed concerns: capability implementation handling session details

## Solution: Metadata-Driven + Runtime-Managed

### Core Principle
**Capabilities declare requirements via metadata; runtime handles complexity.**

## Implementation

### 1. Capability Metadata (Declarative)
```clojure
;; In capability manifest
:mcp_metadata {
  :server_url "https://api.githubcopilot.com/mcp/"
  :server_name "github"
  :tool_name "list_issues"
  :protocol_version "2024-11-05"
}

;; Session management hints (read by runtime)
:mcp_requires_session "auto"          ; auto | true | false
:mcp_auth_env_var "MCP_AUTH_TOKEN"    ; generic env var
:mcp_server_url_override_env "MCP_SERVER_URL"
```

**Values for `:mcp_requires_session`:**
- `"auto"`: Runtime detects based on server response (default)
- `"true"`: Always use session management
- `"false"`: Never use session management (stateless servers)

### 2. Generic Capability Implementation
```clojure
(fn [input]
  (let [default_url "https://api.githubcopilot.com/mcp/"
        env_url (call "ccos.system.get-env" "MCP_SERVER_URL")
        mcp_url (if env_url env_url default_url)
        
        ;; Generic auth token (not provider-specific)
        auth_token (or (get input :auth-token)
                       (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))
        
        ;; Standard MCP JSON-RPC request
        mcp_request {:jsonrpc "2.0"
                     :id "mcp_call"
                     :method "tools/call"
                     :params {:name "tool_name"
                              :arguments input}}
        
        headers (if auth_token
                  {:content-type "application/json"
                   :authorization (str "Bearer " auth_token)}
                  {:content-type "application/json"})]
    
    ;; Direct call - runtime intercepts if session required
    (let [response (call "ccos.network.http-fetch"
                        :method "POST"
                        :url mcp_url
                        :headers headers
                        :body (call "ccos.data.serialize-json" mcp_request))]
      (if (get response :body)
        (let [response_json (call "ccos.data.parse-json" (get response :body))
              result (get response_json :result)]
          result)
        {:error "No response from MCP server" :url mcp_url}))))
```

**Key Points:**
- ✅ No mention of `MCP_USE_SESSION_MGMT`
- ✅ No provider-specific env vars (GITHUB_PAT, etc.)
- ✅ Generic `MCP_AUTH_TOKEN` for all MCP servers
- ✅ Runtime can intercept and manage sessions transparently

### 3. Runtime Session Management (Future)

The runtime will:

```rust
// In CapabilityRegistry::execute_in_microvm
if capability_id == "ccos.network.http-fetch" {
    // Check if this is an MCP call that needs session management
    if let Some(mcp_metadata) = capability.metadata.get("mcp_requires_session") {
        match mcp_metadata.as_str() {
            "true" => {
                // Always use session management
                return self.execute_mcp_with_session(server_url, tool_name, args);
            }
            "auto" => {
                // Try direct call, fallback to session if needed
                match self.execute_http_fetch(args) {
                    Err(e) if e.to_string().contains("Invalid session ID") => {
                        return self.execute_mcp_with_session(server_url, tool_name, args);
                    }
                    result => return result
                }
            }
            "false" | _ => {
                // Direct call, no session management
                return self.execute_http_fetch(args);
            }
        }
    }
}
```

**Session Pool:**
```rust
struct MCPSessionPool {
    sessions: HashMap<String, MCPSession>, // server_url → session
}

struct MCPSession {
    session_id: String,
    initialized_at: Instant,
    last_used: Instant,
    expires_at: Option<Instant>,
}
```

**Lifecycle:**
1. **Initialize** (on first call): Send `initialize` RPC, store `Mcp-Session-Id`
2. **Call** (subsequent calls): Reuse session ID, add `Mcp-Session-Id` header
3. **Terminate** (on timeout/error): Send `notifications/cancelled`, cleanup

### 4. Host Capability (Optional Path)

For explicit session control:
```clojure
;; Capability can optionally call session manager directly
(call "ccos.mcp.call-with-session"
      :server-url mcp_url
      :tool-name "list_issues"
      :arguments input)
```

## Configuration Hierarchy

### User Perspective
```bash
# Option 1: Local MCP server (no auth, no sessions)
export MCP_SERVER_URL=http://localhost:3000/mcp/github

# Option 2: Generic MCP auth (stateless)
export MCP_AUTH_TOKEN=your_token

# Option 3: Session-managed MCP (GitHub Copilot)
export MCP_AUTH_TOKEN=ghp_your_github_pat
# Runtime detects session requirement automatically via metadata
```

### No More Provider-Specific Logic
**Before (❌):**
```bash
export MCP_USE_SESSION_MGMT=true
export GITHUB_PAT=ghp_xxx
```

**After (✅):**
```bash
export MCP_AUTH_TOKEN=ghp_xxx
# Session management is transparent, driven by metadata
```

## Benefits

### 1. **Generic**
- Works with any MCP server (GitHub, custom, local, etc.)
- No hardcoded provider assumptions
- Same pattern for all MCP capabilities

### 2. **Declarative**
- Metadata describes requirements
- Capability code stays simple
- Runtime makes decisions

### 3. **Flexible**
- Supports stateless (local) and stateful (Copilot) servers
- Auto-detection with fallback
- Manual override via metadata

### 4. **Secure**
- No credentials in capability code
- Generic token management
- Runtime enforces permissions

### 5. **Testable**
- Local MCP server: no auth, no sessions
- Mock mode: full control
- Real API: transparent session handling

## Testing Strategies

### Development (Local MCP Server)
```bash
# Start local MCP server (no auth needed)
npx @modelcontextprotocol/server-github start

export MCP_SERVER_URL=http://localhost:3000/mcp/github
cargo run --bin test_github_list_issues
```

### Production (GitHub Copilot API)
```bash
# Runtime detects session requirement from metadata
export MCP_AUTH_TOKEN=$GITHUB_PAT
cargo run --bin demo_call_capabilities
```

### Unit Tests (Mock)
```rust
let env = CCOSBuilder::new()
    .http_mock_mode(true)
    .build()?;
// No network calls, full control
```

## Migration Path

### Phase 1: Current (✅ Complete)
- Generic capability implementation
- Metadata fields defined
- Works with local servers and direct HTTP

### Phase 2: Runtime Enhancement (🚧 Next)
- Read `:mcp_requires_session` metadata
- Implement session pool in `CapabilityRegistry`
- Auto-detect and handle sessions transparently
- Add `ccos.mcp.call-with-session` for explicit control

### Phase 3: Advanced (🔮 Future)
- Session reuse and caching
- Parallel session management
- OAuth flows
- Token refresh

## Example: GitHub List Issues

### Capability File (Generated)
```clojure
(capability "mcp.github.list_issues"
  :name "list_issues"
  :mcp_metadata {
    :server_url "https://api.githubcopilot.com/mcp/"
    :server_name "github"
    :tool_name "list_issues"
  }
  :mcp_requires_session "auto"  ; Runtime decides
  :mcp_auth_env_var "MCP_AUTH_TOKEN"
  :implementation (fn [input] ...))  ; Generic implementation
```

### Usage (User Code)
```clojure
;; Simple call - runtime handles everything
((call "mcp.github.list_issues") {
  :owner "mandubian"
  :repo "ccos"
  :state "open"
})
```

### Behind the Scenes (Runtime)
1. Reads `:mcp_requires_session "auto"` from metadata
2. Attempts direct call to GitHub Copilot MCP API
3. Detects `400 Invalid session ID` error
4. Initiates session: `POST /initialize`
5. Stores `Mcp-Session-Id: abc123`
6. Retries call with session header
7. Returns result
8. Session kept alive for subsequent calls

## Conclusion

**Capabilities stay simple and generic.**
**Metadata describes requirements.**
**Runtime handles complexity.**

This design:
- ✅ Works with any MCP server
- ✅ No hardcoded provider logic
- ✅ Transparent session management
- ✅ Easy to test and deploy
- ✅ Follows CCOS principles

## See Also
- [MCP Generic Auth Design](./MCP_GENERIC_AUTH_DESIGN.md)
- [MCP Session Management Implementation](./MCP_SESSION_MANAGEMENT_IMPLEMENTATION.md)
- [Capability Directory Structure](./CAPABILITY_DIRECTORY_STRUCTURE.md)

