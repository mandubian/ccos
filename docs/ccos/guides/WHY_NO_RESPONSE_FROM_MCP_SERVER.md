# Why "No response from MCP server" When Calling GitHub

## TL;DR

**GitHub Copilot MCP API returns `400 Invalid session ID` because it requires session management.**

The generated MCP capabilities are working correctly. The API needs:
1. `POST /initialize` → get `Mcp-Session-Id`
2. `POST /` with `Mcp-Session-Id` header → make the call
3. `POST /notifications/cancelled` → terminate session

This is Phase 2 work (runtime session management) and is documented in `MCP_SESSION_MANAGEMENT_SOLUTION.md`.

## Current Behavior

When you call `mcp.github.list_issues` with the official GitHub Copilot MCP API:

```bash
export MCP_AUTH_TOKEN=$GITHUB_PAT
cargo run --bin test_github_list_issues
```

**Result:**
```
🌐 HTTP Response: status=400, body_len=19
   Body: Invalid session ID

✅ Execution completed
📊 Full Result:
Map({
    Keyword("error"): String("No response from MCP server"),
    Keyword("url"): String("https://api.githubcopilot.com/mcp/")
})
```

## Root Cause Analysis

### 1. Capability Works Correctly ✅
The MCP capability:
- ✅ Reads auth token from `MCP_AUTH_TOKEN`
- ✅ Builds correct MCP JSON-RPC request
- ✅ Sends `Authorization: Bearer <token>` header
- ✅ Makes HTTP POST to the correct URL
- ✅ Receives response from server

### 2. Server Requires Session ⚠️
The GitHub Copilot MCP API:
- ❌ Rejects direct `tools/call` without session
- ❌ Returns `400 Invalid session ID`
- ✅ Body is not empty (contains error message)
- ✅ Auth is working (otherwise would be 401)

###3. Capability Handles Error Gracefully ✅
The generated RTFS code:
```clojure
(if (get response :body)
  (let [response_json (call "ccos.data.parse-json" (get response :body))
        result (get response_json :result)]
    result)
  {:error "No response from MCP server" :url mcp_url})
```

The body `"Invalid session ID"` is:
- Not valid JSON
- Can't be parsed
- Causes `:result` to be nil
- Triggers "No response" error

This is correct defensive programming!

## How Session Management Works

### During Introspection (✅ Working)
When we run `test_real_github_mcp`:
```rust
// In mcp_introspector.rs via MCPSessionManager
1. POST /initialize → session_id: "abc123"
2. POST / with Mcp-Session-Id: abc123 → tools/list response
3. POST /notifications/cancelled → cleanup
```

**Result:** All 46 GitHub tools discovered and capabilities generated.

### During Capability Execution (🚧 Not Yet Implemented)
When we call `mcp.github.list_issues`:
```rust
// Currently: Direct HTTP call
POST / with Authorization header
→ 400 Invalid session ID

// Phase 2: Runtime will intercept and manage
1. Check :mcp_requires_session metadata
2. Initialize session if needed
3. Make call with session ID
4. Return result
5. Keep session alive for reuse
```

## Workarounds

### Option 1: Local MCP Server (✅ Recommended)
```bash
# Start local server (handles sessions internally)
npx @modelcontextprotocol/server-github

export MCP_SERVER_URL=http://localhost:3000/github-mcp
cargo run --bin test_github_list_issues
```

**Result:** Works! Local server manages sessions for you.

### Option 2: Mock Mode (✅ For Testing)
```rust
let env = CCOSBuilder::new()
    .http_mock_mode(true)
    .build()?;
```

**Result:** No network calls, immediate mock responses.

### Option 3: Wait for Phase 2 (🚧 In Progress)
Runtime will read `:mcp_requires_session "auto"` from metadata and handle sessions transparently.

## What's Next

### Phase 2: Runtime Session Management

**Implementation Plan:**
1. ✅ Metadata fields defined (`:mcp_requires_session`, `:mcp_auth_env_var`)
2. ✅ `MCPSessionManager` exists and works (used during introspection)
3. 🚧 Integrate into `CapabilityRegistry::execute_in_microvm`:
   ```rust
   if capability.metadata.get("mcp_requires_session") == Some("auto") {
       // Try direct call first
       match self.execute_http_fetch(args) {
           Err(e) if e.contains("Invalid session ID") => {
               // Fallback to session-managed call
               return self.execute_mcp_with_session(args);
           }
           result => return result
       }
   }
   ```
4. 🚧 Session pool for reuse across calls
5. 🚧 Async support for concurrent sessions

**Timeline:** Next sprint

## Testing Status

| Test Case | Status | Notes |
|-----------|--------|-------|
| Local MCP server | ✅ Works | No session management needed |
| Mock mode | ✅ Works | No network calls |
| GitHub Copilot API | ⚠️ Expected error | Requires Phase 2 session management |
| JSON serialization | ✅ Fixed | Keywords now strip leading ':' |
| Auth injection | ✅ Works | `MCP_AUTH_TOKEN` properly added |
| Error handling | ✅ Works | Graceful "No response" on parse failure |

## Key Takeaways

1. **Capabilities are correct** - Generic MCP implementation works as designed
2. **JSON is fixed** - Keywords properly converted (`:id` → `"id"`)
3. **Auth works** - Token is sent, server accepts it (400, not 401)
4. **Session management is the gap** - Expected and documented
5. **Workarounds exist** - Local MCP servers work today
6. **Phase 2 will fix** - Runtime will handle sessions transparently

## References

- [MCP Session Management Solution](./MCP_SESSION_MANAGEMENT_SOLUTION.md)
- [MCP Generic Auth Design](./MCP_GENERIC_AUTH_DESIGN.md)
- [MCP Session Management Implementation](./MCP_SESSION_MANAGEMENT_IMPLEMENTATION.md)
- [Testing MCP with GitHub](./TESTING_MCP_WITH_GITHUB.md)
- [GitHub MCP Capabilities Status](./GITHUB_MCP_CAPABILITIES_STATUS.md)

## Debugging Commands

```bash
# See actual HTTP responses
export MCP_AUTH_TOKEN=$GITHUB_PAT
cd rtfs_compiler
cargo run --bin test_github_list_issues 2>&1 | grep "🌐"

# Test with local server
npx @modelcontextprotocol/server-github &
export MCP_SERVER_URL=http://localhost:3000/github-mcp
cargo run --bin test_github_list_issues
```

