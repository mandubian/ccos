# MCP Discovery Tuning (Advanced): Overrides + Scoring

This guide is for **maintainers / power users** who want to tune how CCOS discovers MCP servers when resolving missing capabilities.

If you want to **run CCOS as an MCP server** (so agents like Cursor/Claude can call CCOS tools), see:
- [`ccos-mcp-server.md`](ccos-mcp-server.md)

If you want to **use external MCP tools from RTFS/CCOS**, see:
- [`mcp-runtime-guide.md`](mcp-runtime-guide.md)

## When you need this guide

- You searched for a capability (e.g. `github.search_code`) and discovery returned many servers, but only a few are shown.
- You want to **pin** a known-good server (official/vendor/internal) even if registries are noisy.
- You want to adjust relevance filtering so CCOS shows more/fewer candidates.

## 1) Curated overrides (`capabilities/mcp/overrides.json`)

Curated overrides are **user/maintainer-controlled MCP server entries** merged into discovery results.

Use them to:
- Prefer official servers
- Pin an internal instance URL
- Override registry “junk” results

**Location**:
- `capabilities/mcp/overrides.json`

**Shape** (simplified):
- `entries[]`
  - `matches[]`: patterns that match capability IDs/names (`*` wildcards)
  - `server`: MCP server metadata (name, repo, packages, remotes, env, etc.)

Example:

```json
{
  "entries": [
    {
      "matches": ["github.*", "mcp.github.*", "github.search_code"],
      "server": {
        "name": "github - Official GitHub MCP Server",
        "repository": "https://github.com/github/github-mcp",
        "packages": ["@github/github-mcp"],
        "remotes": [
          { "kind": "websocket", "url": "wss://mcp.github.com" }
        ],
        "env": [
          { "name": "GITHUB_TOKEN", "description": "GitHub PAT for MCP" }
        ]
      }
    }
  ]
}
```

## 2) “Add URL interactively” (the `u` option)

During interactive MCP server selection (e.g., via `resolve-deps`), you can type `u` to:

1. Provide a server WebSocket URL
2. Persist it into `capabilities/mcp/overrides.json`
3. Include it in the current candidate list for immediate selection

**Security guidance**:
- Only add URLs from sources you trust. MCP servers can run code or access data on your behalf.
- Prefer URLs published in official vendor documentation or repositories.

## 3) Relevance scoring & threshold filtering

When you see something like:

```
🔍 DISCOVERY: Found 43 MCP servers for 'github.search_code'
⚠️  UNKNOWN SERVERS: Found 1 server(s) for 'github.search_code'
(Only 1 server scored >= 0.3 relevance threshold from initial discovery)
```

That’s expected: CCOS scores servers and filters out very low relevance matches.

**Scoring algorithm** (`calculate_server_score`, simplified):
- Exact name match: +10
- Exact match in description: +8
- Partial name match: +6
- Reverse partial match: +4
- Description contains capability: +3

**Default threshold**:
- Only servers with `score >= 0.3` are shown

### Tuning the threshold

If you want to show more servers:
- Lower the threshold (e.g. `>= 0.1`)

If you want to be more strict:
- Increase the threshold (e.g. `>= 0.5`)

Recommended long-term:
- Make the threshold configurable (config/CLI) rather than hard-coded.

## Related docs

- [`mcp-runtime-guide.md`](mcp-runtime-guide.md) (runtime/client-side usage)
- [`ccos-mcp-server.md`](ccos-mcp-server.md) (CCOS as MCP server)
- [`server-trust-user-interaction.md`](server-trust-user-interaction.md)

