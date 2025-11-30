# Autonomous Agent MCP Integration Guide

This guide explains how the autonomous agent integrates with real Model Context Protocol (MCP) servers.

## Overview

The autonomous agent now supports **hybrid capability resolution**:
1. **Real MCP Discovery**: Attempts to connect to actual MCP servers and discover tools
2. **Fallback to Mocks**: If MCP connection fails or no server is configured, uses generic mocks

This provides a graceful degradation path and allows testing without requiring real MCP infrastructure.

## Architecture

### Resolution Flow

```
User Goal → Decompose → For each step:
  ├─→ 1. Search Local Capabilities
  ├─→ 2. Try MCP Registry Discovery
  │    ├─→ a. Find MCP Server Config (env vars or config file)
  │    ├─→ b. If found: Create MCPDiscoveryProvider
  │    ├─→ c. Call provider.discover() to get real tools
  │    ├─→ d. Match hint to discovered capability
  │    └─→ e. If success: Register & return | If fail: Continue
  ├─→ 3. Fallback: Install Generic Mock
  └─→ 4. Last Resort: Synthesize with LLM
```

### Key Components

1. **`try_install_from_registry()`**: Main entry point for MCP resolution
2. **`find_mcp_server_config()`**: Detects MCP servers from environment/config
3. **`try_real_mcp_discovery()`**: Connects to real MCP server and discovers tools
4. **`install_generic_mock_capability()`**: Fallback mock generator

## Configuration

### Environment Variables

The agent currently supports environment-based MCP server configuration:

#### GitHub MCP Server

```bash
export GITHUB_MCP_ENDPOINT="https://mcp.github.example.com"
export GITHUB_TOKEN="ghp_your_token_here"
```

When the agent encounters hints like:
- `github.*`
- `repository.*`
- `issue.*`

It will automatically detect and try to connect to the GitHub MCP server.

### Future: Config File Support

In the future, MCP servers will be configurable in `agent_config.toml`:

```toml
[[mcp_servers]]
name = "github-mcp"
endpoint = "https://mcp.github.example.com"
auth_token_env = "GITHUB_TOKEN"
protocol_version = "2024-11-05"
timeout_seconds = 30

[[mcp_servers]]
name = "filesystem-mcp"
endpoint = "http://localhost:3000"
protocol_version = "2024-11-05"
```

## Testing with Real MCP

### Prerequisites

1. **MCP Server**: Running MCP-compliant server (HTTP or SSE endpoint)
2. **Authentication**: Token/credentials if required
3. **Network Access**: Agent can reach the MCP endpoint

### Example: GitHub MCP Server

1. **Set up environment**:
   ```bash
   export GITHUB_MCP_ENDPOINT="https://your-mcp-server.com"
   export GITHUB_TOKEN="your_github_token"
   ```

2. **Run the agent**:
   ```bash
   cargo run --example autonomous_agent_demo -- \
     --goal "List issues in the ccos repository for user mandubian"
   ```

3. **Expected behavior**:
   ```
   🧠 Solving Goal...
   
   👉 Step 1: List issues in repository (Hint: github.list_issues)
       🔍 Searching MCP Registry...
       🔌 Attempting real MCP connection to: github-mcp
       🔎 Found 15 capabilities from MCP server
       ✅ Matched MCP capability: github.list_issues - List issues in a repository
       ✅ Real MCP capability discovered: github.list_issues
       ✅ Resolved Remote (Installed): github.list_issues
   ```

### Example: Fallback to Mock

If no MCP server is configured or connection fails:

```
👉 Step 1: Get weather data (Hint: weather.current)
    🔍 Searching MCP Registry...
    ⚠️  Real MCP connection failed: Connection refused. Falling back to mock.
    🌐 [Demo] Installing generic mock capability: weather.current
    🛠️  Generating generic mock data...
    📦 Installed capability: weather.current
    ✅ Resolved Remote (Installed): weather.current
```

## Implementation Details

### MCPDiscoveryProvider

The agent uses the existing `MCPDiscoveryProvider` from the CCOS capability marketplace:

```rust
let provider = MCPDiscoveryProvider::new(config)?;
let capabilities = provider.discover().await?;
```

This provider:
- Calls `/tools` endpoint on the MCP server
- Calls `/resources` endpoint for data sources
- Converts MCP tool definitions to `CapabilityManifest` objects
- Handles authentication, timeouts, and error cases

### Capability Matching

The agent matches capabilities by:
1. **Exact ID match**: `cap.id.contains(hint)`
2. **Description match**: `cap.description.contains(hint)`

This simple heuristic works well for most cases but can be enhanced with:
- Semantic similarity scoring
- Parameter schema matching
- LLM-based selection

## Extending MCP Support

### Adding a New MCP Server

1. **Update `find_mcp_server_config()`**:
   ```rust
   // In autonomous_agent_demo.rs
   fn find_mcp_server_config(&self, hint: &str, _servers: &[McpServer]) -> Option<MCPServerConfig> {
       // ... existing code ...
       
       // Add your MCP server
       if hint.contains("filesystem") || hint.contains("file") {
           if let Ok(endpoint) = std::env::var("FILESYSTEM_MCP_ENDPOINT") {
               return Some(MCPServerConfig {
                   name: "filesystem-mcp".to_string(),
                   endpoint,
                   auth_token: None,
                   timeout_seconds: 30,
                   protocol_version: "2024-11-05".to_string(),
               });
           }
       }
       
       None
   }
   ```

2. **Set environment variables**:
   ```bash
   export FILESYSTEM_MCP_ENDPOINT="http://localhost:3000"
   ```

3. **Test**:
   ```bash
   cargo run --example autonomous_agent_demo -- \
     --goal "List files in the current directory"
   ```

### Stdio MCP Support (Future)

For local MCP servers running as child processes:

```rust
// Future implementation
pub struct StdioMCPProvider {
    command: String,
    args: Vec<String>,
}

impl StdioMCPProvider {
    pub async fn spawn_and_discover(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        // 1. Spawn process (e.g., `npx @modelcontextprotocol/server-github`)
        // 2. Establish stdio JSON-RPC connection
        // 3. Send initialize request
        // 4. Send tools/list request
        // 5. Parse and return capabilities
    }
}
```

## Troubleshooting

### MCP Connection Fails

**Symptom**: `Real MCP connection failed: Connection refused`

**Solutions**:
1. Verify MCP server is running: `curl $GITHUB_MCP_ENDPOINT/tools`
2. Check network connectivity
3. Verify authentication token is valid
4. Check MCP server logs for errors

### Capability Not Found

**Symptom**: `Real MCP connection succeeded but tool not found`

**Solutions**:
1. List all available tools: `curl $GITHUB_MCP_ENDPOINT/tools`
2. Adjust the capability hint to match actual tool names
3. Improve matching logic in `try_real_mcp_discovery()`

### Agent Falls Back to Mock

**Symptom**: Agent always uses generic mock instead of real MCP

**Possible causes**:
1. Environment variables not set: `echo $GITHUB_MCP_ENDPOINT`
2. Hint doesn't trigger MCP server detection (adjust `find_mcp_server_config()`)
3. MCP discovery fails silently (check logs)

## Benefits

### For Development

- **Fast Iteration**: Test agent logic without real services
- **Reproducible**: Mocks generate consistent data
- **No Dependencies**: Works offline

### For Production

- **Real Data**: Connect to actual MCP servers
- **Graceful Degradation**: Falls back if services unavailable
- **Extensible**: Easy to add new MCP providers

## Next Steps

1. **Test with Real GitHub MCP**: Set up a GitHub MCP server and verify integration
2. **Add More Providers**: Filesystem, Calendar, Email MCP servers
3. **Config File Support**: Move from env vars to `agent_config.toml`
4. **Stdio Support**: Add local process-based MCP tools
5. **Better Matching**: Use semantic similarity for capability selection
