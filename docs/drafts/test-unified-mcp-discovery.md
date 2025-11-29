# Testing the Unified MCP Discovery Service

## Quick Test

Run the test example:

```bash
cargo run --example test_unified_mcp_discovery
```

## What It Tests

The test example (`ccos/examples/test_unified_mcp_discovery.rs`) verifies:

1. **Service Creation**: Creates the unified MCP discovery service
2. **Server Listing**: Lists all configured MCP servers
3. **Tool Discovery**: Discovers tools from each configured server
4. **Manifest Conversion**: Converts discovered tools to capability manifests
5. **Marketplace Integration**: Registers capabilities in the marketplace
6. **Catalog Integration**: Indexes capabilities in the catalog
7. **Caching**: Verifies caching works (second discovery should be faster)

## Configuration

### Option 1: Environment Variables

Set environment variables for MCP servers:

```bash
export GITHUB_MCP_ENDPOINT="https://your-github-mcp-server.com"
export MCP_AUTH_TOKEN="your-auth-token"
```

### Option 2: Configuration File

Add servers to `config/overrides.json`:

```json
{
  "mcp_servers": [
    {
      "name": "github",
      "endpoint": "https://your-github-mcp-server.com",
      "auth_token": "your-token",
      "timeout_seconds": 30,
      "protocol_version": "2024-11-05"
    }
  ]
}
```

## Expected Output

```
🔍 Testing Unified MCP Discovery Service

════════════════════════════════════════════════════════════════════

📦 Step 1: Creating Unified MCP Discovery Service
────────────────────────────────────────────────────────────────────
✅ Unified service created

📋 Step 2: Listing Known Servers
────────────────────────────────────────────────────────────────────
Found 1 configured servers:
  - github (https://api.github.com)

🔍 Step 3: Testing Tool Discovery
────────────────────────────────────────────────────────────────────

  Testing server: github (https://api.github.com)
    🔍 Discovering tools...
    ✅ Discovered 5 tools:
      - list_issues: List issues in a repository
      - get_issue: Get a single issue
      - create_issue: Create a new issue
      - update_issue: Update an issue
      - close_issue: Close an issue
    🔄 Converting first tool to manifest...
    ✅ Created manifest: mcp.github.list_issues
      Name: list_issues
      Provider: MCP(...)
      Has input schema: ✅

🏪 Step 4: Testing with Marketplace and Catalog
────────────────────────────────────────────────────────────────────
  ✅ Created service with marketplace and catalog
  🔍 Discovering and registering tools from: github
  ✅ Discovered 5 tools
    ✅ Registered: mcp.github.list_issues
    ✅ Registered: mcp.github.get_issue
    ...
  📊 Marketplace now has 5 total capabilities
  📚 Catalog search for 'github' returned 5 results

════════════════════════════════════════════════════════════════════
✅ All tests completed successfully!
```

## Manual Testing

You can also test the unified service programmatically:

```rust
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::{MCPServerConfig, DiscoveryOptions};

let service = MCPDiscoveryService::new();

let config = MCPServerConfig {
    name: "test-server".to_string(),
    endpoint: "https://your-server.com".to_string(),
    auth_token: Some("token".to_string()),
    timeout_seconds: 30,
    protocol_version: "2024-11-05".to_string(),
};

let options = DiscoveryOptions {
    introspect_output_schemas: false,
    use_cache: true,
    register_in_marketplace: false,
    auth_headers: None,
};

let tools = service.discover_tools(&config, &options).await?;
println!("Discovered {} tools", tools.len());
```

## Verifying Caching

To verify caching works:

1. Run the test once (tools are discovered and cached)
2. Run it again immediately (should use cache, faster)
3. Check for cache files in the cache directory (if file caching is enabled)

## Troubleshooting

### "No servers configured"

- Add servers to `config/overrides.json` or set environment variables
- See configuration section above

### "Discovery failed"

- Check server endpoint is accessible
- Verify auth token is correct (if required)
- Check network connectivity
- Some servers may require specific headers

### "Cache not working"

- Ensure `use_cache: true` in `DiscoveryOptions`
- Check cache directory permissions (if file caching enabled)
- Cache TTL is 24 hours by default

