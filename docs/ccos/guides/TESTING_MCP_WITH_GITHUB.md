# Testing MCP Introspection with GitHub

## 🎯 Overview

The MCP introspector can discover tools from any MCP server and generate RTFS capabilities. This guide covers how to test with GitHub MCP tools.

## 🚧 Current Status

### GitHub Copilot MCP API

The GitHub Copilot MCP API (`https://api.githubcopilot.com/mcp/`) requires **session-based authentication**, not simple Bearer token auth:

```bash
$ cargo run --bin test_real_github_mcp
Error: Generic("MCP server returned error (400 Bad Request): Invalid session ID\n")
```

**Why this fails:**
- The GitHub Copilot MCP API requires a **session ID** in addition to authentication
- Session IDs are typically obtained through an OAuth or session establishment flow
- Simple Bearer token from `GITHUB_PAT` is not sufficient

### ✅ Recommended Approach: Use Cursor's MCP Server

Cursor (and other MCP clients) provide local MCP servers that handle the complex authentication:

```bash
# Cursor's MCP server configuration (from ~/.cursor/mcp.json)
{
  "mcpServers": {
    "mcp-server-github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "<your-token>"
      }
    }
  }
}
```

## 🧪 Testing Options

### Option 1: Mock Mode (No Server Required)

The introspector has a mock mode for testing without a real MCP server:

```bash
cargo run --bin test_mcp_introspection
```

**Output:**
```
✅ MCP Introspection Complete!
   Discovered 2 tools as capabilities
   
📋 Discovered MCP Tools:
1. create_issue (mcp.github.create_issue)
2. list_issues (mcp.github.list_issues)

💾 Saved: capabilities/mcp.github.create_issue/capability.rtfs
💾 Saved: capabilities/mcp.github.list_issues/capability.rtfs
```

### Option 2: Local MCP Server (Recommended for Real Testing)

Set up a local MCP server that handles GitHub authentication:

#### Step 1: Install MCP GitHub Server

```bash
npm install -g @modelcontextprotocol/server-github
```

#### Step 2: Start the MCP Server

```bash
export GITHUB_PERSONAL_ACCESS_TOKEN=your_github_pat_here
npx @modelcontextprotocol/server-github
```

#### Step 3: Test Introspection

```bash
# Set the local server URL
export GITHUB_MCP_SERVER_URL=http://localhost:3000/github-mcp
export GITHUB_PAT=your_github_pat_here

# Run the test
cargo run --bin test_real_github_mcp
```

### Option 3: Use Cursor's Built-in MCP

If you're using Cursor, it already runs MCP servers for you:

```bash
# Cursor typically runs MCP servers on localhost
# Check your Cursor settings for the actual port and path
export GITHUB_MCP_SERVER_URL=http://localhost:YOUR_PORT/github-mcp
export GITHUB_PAT=your_github_pat_here

cargo run --bin test_real_github_mcp
```

## 📊 What Gets Generated

When introspection succeeds, you get RTFS capabilities like this:

```clojure
(capability "mcp.github.list_issues"
  :name "list_issues"
  :version "1.0.0"
  :description "List issues in a GitHub repository"
  :provider "MCP"
  :input-schema {
    :owner :string
    :repo :string
    :state :string ;; optional
  }
  :output-schema [:vector :map]
  :implementation
    (fn [input]
      (let [mcp_request {:jsonrpc "2.0"
                         :id "mcp_call"
                         :method "tools/call"
                         :params {:name "list_issues"
                                  :arguments input}}
            mcp_url "http://localhost:3000/github-mcp"
            response (call "ccos.network.http-fetch"
                          :method "POST"
                          :url mcp_url
                          :headers {:content-type "application/json"}
                          :body (call "ccos.data.serialize-json" mcp_request))
            response_json (call "ccos.data.parse-json" (get response :body))
            result (get response_json :result)]
        result)))
```

## 🔧 Troubleshooting

### Error: "Invalid session ID"

**Cause:** Trying to connect to GitHub Copilot MCP API directly
**Solution:** Use a local MCP server or mock mode instead

### Error: "Connection refused"

**Cause:** No MCP server running on the specified URL
**Solution:** 
1. Start a local MCP server, or
2. Use mock mode for testing, or
3. Verify the correct URL/port for your MCP server

### Error: "401 Unauthorized"

**Cause:** Missing or invalid authentication
**Solution:** 
1. Ensure `GITHUB_PAT` environment variable is set
2. Verify the token has correct permissions (repo access)
3. Check that the MCP server is configured to use the token

## 🎯 Key Takeaways

1. **Direct GitHub Copilot MCP API access requires session management** - not yet implemented
2. **Local MCP servers handle authentication complexity** - recommended approach
3. **Mock mode works without any server** - great for testing the introspection itself
4. **Generated capabilities work identically** regardless of how they were discovered

## 🚀 Next Steps

Once you have capabilities generated (via mock or real server):

```bash
# Test calling the generated capability
cargo run --bin call_mcp_github
```

This will demonstrate that the generated RTFS capabilities work correctly, loading and executing the MCP tool calls!


