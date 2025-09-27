# GitHub MCP Capability Registry Demo

This guide shows how to bootstrap the CCOS `CapabilityRegistry` with tools
exposed by the hosted [GitHub MCP Server](https://github.com/github/github-mcp-server).
The companion example lives at `rtfs_compiler/examples/github_mcp_registry.rs`
and uses only synchronous HTTP calls, making it easy to experiment from the
command line.

## Prerequisites

1. **GitHub Personal Access Token (PAT)** with scopes that allow the hosted MCP
   server to enumerate repository metadata (`repo` is sufficient for read-only
   queries).
2. Rust toolchain with `cargo` (the example compiles as part of the existing
   workspace).
3. Network access to `https://api.githubcopilot.com`.

Export the PAT before running the demo:

```bash
export GITHUB_PERSONAL_ACCESS_TOKEN="ghp_your_token_here"
```

## Running the Example

List the first 10 read-only repository tools and stop without executing any of
them:

```bash
cargo run --example github_mcp_registry -- \
  --toolset repos \
  --readonly \
  --limit 10 \
  --list-only
```

Execute a specific tool after registration (the tool identifier matches the
pattern `github.mcp.<tool_name>`). The payload is forwarded verbatim to the MCP
server as JSON arguments:

```bash
cargo run --example github_mcp_registry -- \
  --toolset repos \
  --tool github.mcp.repos_get_repository \
  --payload '{"owner":"github","repo":"github-mcp-server"}'
```

### Command Flags

| Flag | Description |
| --- | --- |
| `--endpoint` | Override the MCP base URL (defaults to the hosted server). |
| `--toolset` | Select a single toolset (`repos`, `issues`, `actions`, …). |
| `--readonly` | Append `/readonly` to the endpoint for read-only tooling. |
| `--token` | Provide a PAT inline instead of using the environment variable. |
| `--timeout-seconds` | HTTP timeout for discovery and tool execution. |
| `--limit` | Cap the number of registered tools (prevents huge registries). |
| `--refresh` | Ignore the cache and fetch a fresh MCP tool catalog. |
| `--no-cache` | Skip loading or saving any cached catalogs. |
| `--cache-ttl-seconds` | Maximum age for cached catalogs before they expire (0 = no expiration). |
| `--cache-dir` | Custom directory for cached catalogs (defaults to the XDG cache hierarchy). |
| `--tool` | Execute the named capability after registration. |
| `--payload` | JSON payload forwarded to the selected tool. |
| `--list-only` | Skip execution and simply print the registered capability IDs. |

## Initialization & Session Handshake

Before listing tools, the example issues an MCP `initialize` request to the base
endpoint. This advertises minimal client capabilities, surfaces any server
instructions, and records the `Mcp-Session-Id` response header when present.
That header is echoed on subsequent RPC calls. If the server does not emit a
header, the demo attempts to create a session via `session/create`. Some MCP
deployments decline that handshake; the demo logs the refusal and continues with
sessionless JSON-RPC calls. When either the header or the `session/create`
response yields an ID, the code automatically includes it in the JSON params and
HTTP headers for every `tools/list` and `tools/call` request.

## Catalog Caching

Each MCP catalog is cached on disk (under `~/.cache/ccos/` or
`$XDG_CACHE_HOME/ccos/`, with filenames prefixed `github_mcp_registry_`) using
the endpoint, toolset, and read-only flag as part of the cache key. On startup,
the example tries to load a
matching cache entry; when one is found and it is younger than the configured
TTL (defaults to one hour), the tools are registered without issuing a new
`tools/list`. Otherwise, the example fetches the catalog from the MCP server and
stores it back to disk. Pass `--refresh` to force a new discovery, `--no-cache`
to bypass persistence, or adjust `--cache-ttl-seconds` to tune the expiration.

## How It Works

1. Build the effective endpoint from the base URL, optional toolset, and
   read-only toggle (e.g. `https://api.githubcopilot.com/mcp/x/repos/readonly`).
2. POST a JSON-RPC `tools/list` request to fetch tool definitions (unless a
   valid cache entry already supplies them), optionally including the session
   header captured during initialization.
3. For each tool, construct a CCOS `Capability` whose implementation issues a
   JSON-RPC `tools/call` request back to the GitHub MCP server (again reusing
   the session header when available).
4. Register those capabilities in the runtime `CapabilityRegistry` via the new
   `register_custom_capability` helper.
5. Optionally execute one of the registered capabilities to verify the flow.

All conversions between RTFS `Value` instances and JSON payloads are handled in
Rust, so RTFS programs can call the registered capabilities without knowing the
MCP wire format.

## Troubleshooting

- **`Invalid session ID`**: Ensure the PAT is valid and that the MCP server
   accepts sessionless requests. When the server rejects `session/create`, the
   demo captures any `Mcp-Session-Id` header from `initialize`. If calls still
   fail, double-check the header by running the `curl` snippet in this guide and
   confirm the token is echoed back on subsequent requests.
- **Unexpected tool listings**: Run with `--refresh` to force a fresh MCP
   catalog fetch, or `--no-cache` to bypass the on-disk cache entirely.
- **`405 Method Not Allowed`**: Indicates the server expects JSON-RPC POSTs; the
   demo already uses POST, so double-check custom `--endpoint` overrides.
