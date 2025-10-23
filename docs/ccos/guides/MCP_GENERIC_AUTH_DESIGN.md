# MCP Generic Authentication & Session Management Design

## Overview

This document describes the **generic, provider-agnostic** approach to handling authentication and session management for MCP (Model Context Protocol) capabilities in CCOS.

## Design Principles

### 1. **Generic by Default**
- MCP capability generation should work for **any** MCP server
- No hardcoded provider-specific logic (GitHub, OpenWeather, etc.)
- Configuration through metadata and environment variables

### 2. **Separation of Concerns**
- **Capability**: Pure business logic - what the tool does
- **Metadata**: Declarative hints about auth/session requirements
- **Runtime/Registry**: Handles session management transparently
- **User/Deployment**: Provides credentials via environment variables

### 3. **Flexibility**
- Supports different auth mechanisms (Bearer tokens, API keys, custom headers)
- Works with both stateless (local MCP servers) and stateful (GitHub Copilot API) servers
- Allows per-capability and per-server configuration

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     MCP Capability (.rtfs)                    │
│                                                               │
│  (fn [input]                                                 │
│    ;; Generic implementation:                                │
│    ;; 1. Read server URL from metadata/env                   │
│    ;; 2. Optional: get auth token from input/:auth-token     │
│    ;;    or env var MCP_AUTH_TOKEN                          │
│    ;; 3. Make standard JSON-RPC call                         │
│    ;; 4. Return result                                       │
│    ...)                                                      │
└──────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────┐
│                   Capability Metadata                         │
│                                                               │
│  :mcp_metadata {                                             │
│    :server_url "https://api.example.com/mcp/"               │
│    :server_name "github"                                    │
│    :tool_name "list_issues"                                 │
│    :protocol_version "2024-11-05"                           │
│  }                                                          │
│  :mcp_requires_session "auto"      ; auto, true, false     │
│  :mcp_auth_env_var "MCP_AUTH_TOKEN" ; generic env var      │
│  :mcp_server_url_override_env "MCP_SERVER_URL"             │
└──────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────┐
│                  Runtime / Registry                           │
│                                                               │
│  - Reads :mcp_requires_session metadata                      │
│  - If "auto" or "true": manages session lifecycle            │
│  - Maintains session pool per server                         │
│  - Injects Mcp-Session-Id headers transparently              │
│  - Handles initialize/call/terminate flow                    │
└──────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────┐
│                  Environment Variables                        │
│                                                               │
│  - MCP_SERVER_URL: Override default server URL               │
│  - MCP_AUTH_TOKEN: Generic auth token for MCP servers        │
│  - Provider-specific can still be used via input schema      │
└──────────────────────────────────────────────────────────────┘
```

## Configuration Options

### Option 1: Local MCP Server (Recommended for Development)
```bash
export MCP_SERVER_URL=http://localhost:3000/mcp/github
# No auth needed for local server
```

**Behavior**: Direct JSON-RPC calls, no session management

### Option 2: Generic MCP Server with Auth
```bash
export MCP_SERVER_URL=https://some-mcp-server.com/api/
export MCP_AUTH_TOKEN=your_token_here
```

**Behavior**: Adds `Authorization: Bearer <token>` header to all requests

### Option 3: Per-Call Auth (via Input Schema)
```clojure
((call "mcp.github.list_issues") {
  :owner "mandubian"
  :repo "ccos"
  :auth-token "temporary_token"  ; overrides MCP_AUTH_TOKEN
})
```

**Behavior**: Token passed directly in the capability call

### Option 4: Session-Managed (GitHub Copilot API)
```bash
export MCP_USE_SESSION_MGMT=true
export MCP_AUTH_TOKEN=your_github_pat
# Or provider-specific: GITHUB_PAT
```

**Behavior**: Runtime handles initialize → call → terminate flow

## Implementation Details

### Generated RTFS Capability Structure

```clojure
(fn [input]
  ;; 1. Configuration: read from metadata or env
  (let [default_url "https://api.githubcopilot.com/mcp/"
        env_url (call "ccos.system.get-env" "MCP_SERVER_URL")
        mcp_url (if env_url env_url default_url)
        
        ;; 2. Optional auth token (from input or env)
        auth_token (or (get input :auth-token)
                       (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))
        
        ;; 3. Build standard MCP JSON-RPC request
        mcp_request {:jsonrpc "2.0"
                     :id "mcp_call"
                     :method "tools/call"
                     :params {:name "list_issues"
                              :arguments input}}
        
        ;; 4. Build headers with optional auth
        headers (if auth_token
                  {:content-type "application/json"
                   :authorization (str "Bearer " auth_token)}
                  {:content-type "application/json"})]
    
    ;; 5. Make HTTP POST to MCP server
    (let [response (call "ccos.network.http-fetch"
                        :method "POST"
                        :url mcp_url
                        :headers headers
                        :body (call "ccos.data.serialize-json" mcp_request))]
      
      ;; 6. Parse and return result
      (if (get response :body)
        (let [response_json (call "ccos.data.parse-json" (get response :body))
              result (get response_json :result)]
          result)
        {:error "No response from MCP server" :url mcp_url}))))
```

### Capability Metadata Fields

```clojure
:mcp_metadata {
  :server_url "https://api.githubcopilot.com/mcp/"  ; Default MCP server URL
  :server_name "github"                              ; Server namespace
  :tool_name "list_issues"                           ; Tool name
  :protocol_version "2024-11-05"                     ; MCP protocol version
}

;; Session management hints (for future runtime enhancement)
:mcp_requires_session "auto"          ; "auto" | "true" | "false"
:mcp_auth_env_var "MCP_AUTH_TOKEN"    ; Generic env var name
:mcp_server_url_override_env "MCP_SERVER_URL"  ; URL override env var
```

## Future Enhancements

### Phase 1: Current (✅ Complete)
- Generic RTFS implementation
- Optional auth via input schema or env var
- Metadata-based configuration
- Works with local MCP servers

### Phase 2: Runtime Session Management (🚧 In Progress)
- `ccos.mcp.call-with-session` host capability
- Transparent session lifecycle management
- Session pooling and reuse
- Auto-detection based on `:mcp_requires_session` metadata

```clojure
;; Future capability implementation will optionally use:
(if (requires-session? metadata)
  (call "ccos.mcp.call-with-session"
        :server-url mcp_url
        :tool-name "list_issues"
        :arguments input)
  ;; Direct call for stateless servers
  (call "ccos.network.http-fetch" ...))
```

### Phase 3: Advanced Auth Patterns
- OAuth 2.0 flows
- API key injection at different locations (header, query, body)
- Multi-factor authentication
- Token refresh and rotation

## Security Considerations

### 1. **No Hardcoded Credentials**
- Never embed tokens/keys in capability code
- Always read from environment or secure stores

### 2. **Token Hierarchy**
```
1. Per-call :auth-token (highest priority)
2. MCP_AUTH_TOKEN (generic env var)
3. Provider-specific env var (e.g., GITHUB_PAT)
4. No auth (local development)
```

### 3. **Capability Permissions**
All MCP capabilities require:
```clojure
:permissions [:network.http]
:effects [:network_request :mcp_call]
```

Runtime enforces these before execution.

### 4. **Host Allowlists**
When using `ccos.network.http-fetch`, respect runtime's HTTP allowlist:
```rust
// In registry.rs
if let Some(allow_hosts) = &self.http_allow_hosts {
    // Check host before making request
}
```

## Testing Strategies

### 1. Local MCP Server (Recommended)
```bash
# Start local MCP server
npx @modelcontextprotocol/server-github start

# Set URL
export MCP_SERVER_URL=http://localhost:3000/mcp/github

# Test capability
cargo run --bin test_github_list_issues
```

### 2. Mock Server (Unit Tests)
```rust
// Use http_mocking_enabled in tests
let env = CCOSBuilder::new()
    .http_mock_mode(true)
    .build()?;
```

### 3. Real API with Auth
```bash
export MCP_AUTH_TOKEN=your_real_token
cargo run --bin demo_call_capabilities
```

## Migration Notes

### From Provider-Specific to Generic

**Before** (❌ Provider-specific):
```clojure
(let [github_pat (call "ccos.system.get-env" "GITHUB_PAT")  ; GitHub-specific
      headers {:authorization (str "Bearer " github_pat)}]
  ...)
```

**After** (✅ Generic):
```clojure
(let [auth_token (or (get input :auth-token)
                     (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))  ; Generic
      headers (if auth_token
                {:authorization (str "Bearer " auth_token)}
                {})]
  ...)
```

### Backwards Compatibility

Users can still use provider-specific env vars by explicitly passing them:
```bash
# Provider-specific env var (still works)
export GITHUB_PAT=ghp_xxx

# In custom wrapper or deployment script:
export MCP_AUTH_TOKEN=$GITHUB_PAT
```

## Best Practices

### For Capability Authors
1. ✅ Use generic `MCP_AUTH_TOKEN` env var
2. ✅ Allow `:auth-token` in input schema (optional field)
3. ✅ Read server URL from metadata with env override
4. ✅ Use standard MCP JSON-RPC format
5. ❌ Don't hardcode provider-specific logic

### For Capability Users
1. ✅ Set `MCP_SERVER_URL` for local testing
2. ✅ Use `MCP_AUTH_TOKEN` for generic auth
3. ✅ Check capability metadata for specific requirements
4. ✅ Prefer environment variables over inline tokens
5. ❌ Don't commit credentials to source control

### For Runtime Developers
1. ✅ Read `:mcp_requires_session` from metadata
2. ✅ Implement session pooling for efficiency
3. ✅ Handle session errors gracefully
4. ✅ Log session lifecycle events for debugging
5. ❌ Don't leak session IDs in error messages

## Examples

### Example 1: GitHub List Issues (Local Server)
```bash
# Terminal 1: Start local MCP server
npx @modelcontextprotocol/server-github start

# Terminal 2: Test capability
export MCP_SERVER_URL=http://localhost:3000/mcp/github
cargo run --bin test_github_list_issues
```

### Example 2: GitHub with Copilot API
```bash
export MCP_AUTH_TOKEN=$GITHUB_PAT
cargo run --bin test_real_github_mcp
```

### Example 3: Custom MCP Server
```bash
export MCP_SERVER_URL=https://my-mcp-server.com/api/v1/
export MCP_AUTH_TOKEN=my_custom_token
((call "mcp.custom.my_tool") {:param1 "value1"})
```

## Conclusion

This generic approach provides:
- ✅ **Flexibility**: Works with any MCP server
- ✅ **Security**: No hardcoded credentials
- ✅ **Simplicity**: Standard patterns for all MCP capabilities
- ✅ **Extensibility**: Easy to add session management, OAuth, etc.
- ✅ **Testability**: Multiple testing strategies supported

The design follows CCOS principles:
- **Declarative**: Metadata describes requirements
- **Composable**: Capabilities work with any MCP server
- **Secure**: Runtime enforces permissions and validates inputs
- **Observable**: Clear configuration via env vars and metadata

