# MCP Runtime Guide: Using MCP Capabilities in CCOS

## Overview

This guide explains how to **use** MCP (Model Context Protocol) capabilities in CCOS, including session management, authentication, and calling MCP tools from RTFS code.

> If you’re looking to **run CCOS as an MCP server** (so Cursor/Claude/any MCP client can call CCOS tools), see:
> - [`ccos-mcp-server.md`](ccos-mcp-server.md)

## Quick Start

### Prerequisites

1. MCP capabilities generated in `capabilities/mcp/` (see `capability-synthesis.md`)
2. Environment variable for authentication: `MCP_AUTH_TOKEN`

### Basic Usage

```rtfs
;; Call GitHub MCP capabilities
(call "mcp.github.get_me" {})

(call "mcp.github.list_issues" {
  :owner "mandubian"
  :repo "ccos"
  :state "OPEN"  ;; Note: GitHub expects uppercase OPEN/CLOSED
})

(call "mcp.github.create_issue" {
  :owner "mandubian"
  :repo "ccos"
  :title "New Feature Request"
  :body "Description of the feature..."
})
```

**Session management is automatic!** No manual session handling needed.

## Session Management

### How It Works

MCP capabilities automatically use session management when metadata indicates it's required:

```rtfs
;; In the .rtfs capability file:
:metadata {
  :mcp {
    :requires_session "auto"        ; Triggers automatic session management
    :server_url "https://api.githubcopilot.com/mcp/"
    :auth_env_var "MCP_AUTH_TOKEN"  ; Where to find auth token
  }
}
```

### Execution Flow

```
First call to mcp.github.get_me:
  📋 Detects requires_session = "auto" from metadata
  🔌 Initializes MCP session
     - POST /initialize with protocol version
     - Receives Mcp-Session-Id header
     - Stores session in pool
  🔧 Calls tools/call with session
     - Adds Mcp-Session-Id header
     - Adds Authorization header (from MCP_AUTH_TOKEN)
  ✅ Returns result

Second call to mcp.github.list_issues:
  ♻️  Reuses existing session (from pool)
  🔧 Calls tools/call with same session
  ✅ Returns result
```

### Session Pooling

Sessions are automatically:
- **Initialized** on first use
- **Pooled** per capability
- **Reused** across multiple calls
- **Thread-safe** with proper locking

### Authentication

**Environment Variable**: `MCP_AUTH_TOKEN`

```bash
# Set GitHub Personal Access Token
export MCP_AUTH_TOKEN="ghp_your_token_here"

# Now all GitHub MCP calls will be authenticated
```

The runtime automatically:
1. Reads `MCP_AUTH_TOKEN` from environment
2. Injects as `Authorization: Bearer <token>` header
3. Includes in all MCP requests

### Server URL Configuration

**Default**: From capability metadata (`:mcp {:server_url "..."}`

**Override**: Set `MCP_SERVER_URL` environment variable

```bash
# Use custom MCP server
export MCP_SERVER_URL="http://localhost:3000/mcp"

# All MCP capabilities will use this URL instead of metadata default
```

## MCP Protocol Details

### Session Initialization

When a capability with `:requires_session "auto"` is called:

**Request**:
```json
POST https://api.githubcopilot.com/mcp/
Content-Type: application/json
Authorization: Bearer <MCP_AUTH_TOKEN>

{
  "jsonrpc": "2.0",
  "id": "init",
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "clientInfo": {
      "name": "ccos-rtfs",
      "version": "0.1.0"
    },
    "capabilities": {}
  }
}
```

**Response**:
```
Mcp-Session-Id: 57d9f5e2-cc0f-4170-9740-480d9ee51106

{
  "jsonrpc": "2.0",
  "id": "init",
  "result": { ... }
}
```

### Tool Execution

**Request**:
```json
POST https://api.githubcopilot.com/mcp/
Content-Type: application/json
Authorization: Bearer <MCP_AUTH_TOKEN>
Mcp-Session-Id: 57d9f5e2-cc0f-4170-9740-480d9ee51106

{
  "jsonrpc": "2.0",
  "id": "tool_call",
  "method": "tools/call",
  "params": {
    "name": "list_issues",
    "arguments": {
      "owner": "mandubian",
      "repo": "ccos",
      "state": "OPEN"
    }
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": "tool_call",
  "result": {
    "issues": [...],
    "totalCount": 130
  }
}
```

## Capability Metadata

MCP capabilities use hierarchical metadata:

```rtfs
:metadata {
  :mcp {
    :server_url "https://api.githubcopilot.com/mcp/"
    :server_name "github"
    :tool_name "list_issues"
    :protocol_version "2024-11-05"
    :requires_session "auto"
    :auth_env_var "MCP_AUTH_TOKEN"
    :server_url_override_env "MCP_SERVER_URL"
  }
  :discovery {
    :method "mcp_introspection"
    :source_url "https://api.githubcopilot.com/mcp/"
    :created_at "2025-10-23T21:35:43Z"
    :capability_type "mcp_tool"
  }
}
```

At runtime, this is flattened to:
```rust
HashMap {
  "mcp_server_url" => "https://api.githubcopilot.com/mcp/",
  "mcp_requires_session" => "auto",
  "mcp_auth_env_var" => "MCP_AUTH_TOKEN",
  ...
}
```

## Troubleshooting

### "401 Unauthorized"

**Cause**: Missing or invalid `MCP_AUTH_TOKEN`

**Solution**:
```bash
# Check if token is set
echo $MCP_AUTH_TOKEN

# Set GitHub PAT
export MCP_AUTH_TOKEN="ghp_your_personal_access_token"
```

### "No response from MCP server"

**Causes**:
1. Server URL incorrect
2. Network connectivity issues
3. Server requires session but capability doesn't declare it

**Debug**:
- Check logs for "📋 Metadata indicates session management required"
- Verify `:requires_session "auto"` in capability metadata
- Test server URL with curl

### "Invalid session ID"

**Cause**: Session expired or server restarted

**Solution**: Restart application (sessions are in-memory)

**Future**: Automatic session refresh on 401/session errors

### "Expected OPEN, got open"

**Cause**: GitHub MCP uses uppercase enum values (GraphQL convention)

**Solution**: Use `"OPEN"` and `"CLOSED"`, not `"open"` and `"closed"`

## Architecture

### Generic Session Management

The session management is **completely provider-agnostic**:

```rust
// Runtime checks metadata generically
if metadata.get("mcp_requires_session") == Some("auto") {
    // Delegate to SessionPoolManager
    session_pool.execute_with_session(...)
}

// SessionPoolManager routes to MCPSessionHandler
// MCPSessionHandler implements MCP protocol
```

**Zero MCP-specific code** in registry or marketplace!

### Components

1. **SessionPoolManager** (generic)
   - Routes to provider-specific handlers
   - Detects provider from metadata keys (`mcp_*`, `graphql_*`, etc.)

2. **MCPSessionHandler** (MCP-specific)
   - Implements MCP protocol (initialize, execute, terminate)
   - Manages session pool
   - Injects auth tokens

3. **Marketplace/Registry**
   - Detects session requirements from metadata
   - Delegates to session pool
   - Zero knowledge of MCP protocol

## Testing

### Verify Session Management Works

```bash
# Set auth token
export MCP_AUTH_TOKEN="your_github_pat"

# Run end-to-end test
cd rtfs_compiler
cargo run --bin test_end_to_end_session
```

**Expected output**:
```
🔌 Initializing MCP session with https://api.githubcopilot.com/mcp/
✅ MCP session initialized: <session-id>
🔧 Calling MCP tool: get_me with session <session-id>
✅ Capability executed successfully
🎉 SUCCESS! Got user data from GitHub API
```

### Run Unit Tests

```bash
cd rtfs_compiler
cargo test --lib session_pool
```

**Expected**: `test result: ok. 3 passed; 0 failed`

## Available GitHub MCP Capabilities

Generated capabilities are in `capabilities/mcp/github/*.rtfs` (46 tools):

**Repository Management**:
- `create_repository`, `fork_repository`
- `create_branch`, `list_branches`
- `list_commits`, `get_commit`

**Issues**:
- `list_issues`, `get_issue`, `create_issue`, `update_issue`
- `add_issue_comment`, `get_issue_comments`
- `add_sub_issue`, `remove_sub_issue`, `list_sub_issues`

**Pull Requests**:
- `list_pull_requests`, `create_pull_request`, `update_pull_request`
- `merge_pull_request`, `update_pull_request_branch`
- Pull request reviews and comments

**Search**:
- `search_code`, `search_issues`, `search_pull_requests`
- `search_repositories`, `search_users`

**Files**:
- `get_file_contents`, `create_or_update_file`, `delete_file`
- `push_files`

**Releases & Tags**:
- `list_releases`, `get_latest_release`, `get_release_by_tag`
- `list_tags`, `get_tag`

**Users & Teams**:
- `get_me`, `get_teams`, `get_team_members`

**Other**:
- `assign_copilot_to_issue`, `request_copilot_review`
- `add_comment_to_pending_review`

## Best Practices

### 1. Always Set Auth Token
```bash
export MCP_AUTH_TOKEN="your_token"
```

### 2. Use Correct Enum Values
GitHub uses GraphQL conventions (uppercase):
- State: `"OPEN"`, `"CLOSED"` (not `"open"`, `"closed"`)
- Direction: `"ASC"`, `"DESC"`

### 3. Check Capability Metadata
```rtfs
;; Verify what parameters are expected
;; Look at the capability's :input-schema in the .rtfs file
```

### 4. Handle Errors Gracefully
```rtfs
(let [result (call "mcp.github.list_issues" {...})]
  (if (get result :error)
    (println "Error:" (get result :error))
    (println "Success:" result)))
```

## Environment Variables Summary

| Variable | Purpose | Required | Default |
|----------|---------|----------|---------|
| `MCP_AUTH_TOKEN` | Authentication token (e.g., GitHub PAT) | Yes (for authenticated APIs) | None |
| `MCP_SERVER_URL` | Override MCP server URL | No | From capability metadata |

## Related Documentation

- **Creating MCP capabilities**: `mcp-synthesis-guide.md`
- **Session management architecture**: `session-management-architecture.md`
- **Phase 2 overview**: `metadata-driven-capabilities.md`

---

**Status**: Production Ready ✅  
**Verified**: Real GitHub API calls working  
**Session Management**: Automatic  

