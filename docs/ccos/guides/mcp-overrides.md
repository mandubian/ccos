# Curated MCP Overrides and Interactive Add URL

This guide explains how CCOS resolves MCP servers using curated overrides and how to add a new server URL interactively during capability resolution.

## What are curated overrides?

- Curated overrides are user/maintainer-controlled MCP server entries that are merged into discovery results when resolving a capability.
- Use them to prefer known-good or official servers when public registries don’t list them yet, or when you want to pin to a specific source.

Location:
- File: `capabilities/mcp/overrides.json`

Schema (simplified):
- `entries[]` — list of curated entries
  - `matches[]` — patterns to match capability IDs or names (supports simple `*` wildcards)
  - `server` — MCP server metadata (name, repository, packages, remotes, env, etc.)

Example snippet:
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

Notes:
- The discovery flow merges servers from the registry with any curated matches from `overrides.json`.
- Ranking is biased to favor official-looking signals (e.g., GitHub repo domain, package scope, name hints).

## Add URL interactively (the 'u' option)

During interactive MCP server selection (e.g., via `resolve-deps`), you can type `u` to:
1. Provide a server WebSocket URL.
2. Persist it immediately into `capabilities/mcp/overrides.json` (creating/merging if needed).
3. Include it in the current candidate list for immediate selection.

This is useful when:
- You know a trusted server URL (from vendor docs or your infra) that isn’t listed.
- You want to pin to a specific instance and make it available to future runs.

Security guidance:
- Only add URLs from sources you trust. MCP servers can run code or access data on your behalf.
- Prefer URLs published in official vendor documentation or repositories.
- Consider setting stricter trust policies when trying unknown sources.

## Behavior details

- Pattern matching: supports simple `*` wildcards (e.g., `github.*`) against capability IDs/names.
- Ranking: curated entries are merged and ranked; official/curated signals get a boost so they surface near the top.
- Persistence: URLs added via `u` are saved to `overrides.json` for future sessions.

## Troubleshooting

- If your override doesn’t appear:
  - Verify that one of your `matches[]` patterns matches the capability ID you’re resolving.
  - Ensure `overrides.json` is valid JSON.
  - Run a build to confirm there are no typos that gate execution (`cargo check`).
- If multiple entries seem similar, use the interactive numeric selection to pick precisely the one you want.

## Maintenance tips

- Keep `overrides.json` small and focused on truly curated entries.
- For teams, review overrides in code review to maintain trust hygiene.
- If a public registry adds the official entry later, you can remove the local override.
