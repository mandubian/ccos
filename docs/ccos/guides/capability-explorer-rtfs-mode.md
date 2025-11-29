# Capability Explorer RTFS Mode

The Capability Explorer provides an interactive TUI and an RTFS command-line mode for discovering, inspecting, and testing MCP capabilities.

## Overview

RTFS mode allows you to:
- Discover capabilities from MCP servers using RTFS expressions
- Call MCP tools directly from the command line
- Chain multiple operations with `(do ...)` expressions
- Script discovery and testing workflows

## Quick Start

```bash
# List available MCP servers
cargo run --example capability_explorer -- \
  --rtfs "(call :ccos.discovery.servers {})" \
  --config ../config/agent_config.toml

# Discover capabilities from GitHub MCP server
cargo run --example capability_explorer -- \
  --rtfs "(call :ccos.discovery.discover {:server \"github\"})" \
  --config ../config/agent_config.toml

# Chain discovery + call in one command
cargo run --example capability_explorer -- \
  --rtfs '(do 
    (call :ccos.discovery.discover {:server "github"}) 
    (call :mcp.github.get_me {}))' \
  --config ../config/agent_config.toml
```

## CLI Options

| Option | Description |
|--------|-------------|
| `--rtfs <expr>` | Execute an RTFS expression directly |
| `--rtfs-file <path>` | Execute RTFS expressions from a file |
| `--output-format <format>` | Output format: `rtfs` (default) or `json` |
| `--quiet`, `-q` | Suppress status messages, output only results |
| `--config <path>` | Path to agent configuration file |

## Built-in Discovery Capabilities

### `ccos.discovery.servers`

Lists all configured MCP servers.

```rtfs
(call :ccos.discovery.servers {})
```

**Output:**
```rtfs
[
  {:name "github" :endpoint "https://api.githubcopilot.com/mcp/"}
]
```

### `ccos.discovery.discover`

Discovers capabilities from a specific MCP server and exports them to RTFS files.

```rtfs
(call :ccos.discovery.discover {:server "github"})
```

**Parameters:**
- `:server` (required) - Server name or endpoint URL
- `:hint` (optional) - Filter hint for discovery

**Output:** Vector of discovered capability IDs.

**RTFS Export:** Discovered capabilities are automatically exported to:
```
capabilities/discovered/mcp/<server-name>/capabilities.rtfs
```

This location can be overridden with the `CCOS_CAPABILITY_STORAGE` environment variable.

### `ccos.discovery.search`

Searches discovered capabilities by keyword.

```rtfs
(call :ccos.discovery.search {:hint "issues"})
```

**Parameters:**
- `:hint` (required) - Search keyword

**Output:** Vector of matching capabilities with scores.

### `ccos.discovery.list`

Lists all discovered capabilities.

```rtfs
(call :ccos.discovery.list {})
```

**Output:** Vector of capability info maps with `:id`, `:name`, `:server`.

### `ccos.discovery.inspect`

Gets detailed information about a capability.

```rtfs
(call :ccos.discovery.inspect {:id "mcp.github.list_issues"})
```

**Parameters:**
- `:id` (required) - Capability ID to inspect

**Output:** Detailed capability manifest including input/output schemas.

## Calling MCP Capabilities

After discovering capabilities, call them directly:

```rtfs
(call :mcp.github.get_me {})

(call :mcp.github.list_issues {
  :owner "mandubian"
  :repo "ccos"
  :perPage 5
})

(call :mcp.github.search_repositories {
  :query "language:rust stars:>1000"
  :perPage 10
})
```

## Chaining Commands with `(do ...)`

Use `(do ...)` to execute multiple expressions in sequence:

```rtfs
(do
  ;; First discover capabilities
  (call :ccos.discovery.discover {:server "github"})
  
  ;; Then call a capability
  (call :mcp.github.list_issues {
    :owner "mandubian"
    :repo "ccos"
    :perPage 3
  }))
```

The result of the last expression is returned.

## RTFS Value Syntax

### Maps
```rtfs
{:key1 "value1" :key2 42 :key3 true}
```

### Vectors
```rtfs
["item1" "item2" "item3"]
```

### Keywords
```rtfs
:keyword-name
```

### Strings
```rtfs
"hello world"
"with \"escapes\""
```

### Numbers
```rtfs
42        ;; integer
3.14      ;; float
-100      ;; negative
```

### Booleans
```rtfs
true
false
```

### Nil
```rtfs
nil
```

## Output Formats

### RTFS Format (default)

```bash
cargo run --example capability_explorer -- \
  --rtfs "(call :ccos.discovery.servers {})" \
  --config ../config/agent_config.toml
```

Output:
```rtfs
[
  {:name "github" :endpoint "https://api.githubcopilot.com/mcp/"}
]
```

### JSON Format

```bash
cargo run --example capability_explorer -- \
  --rtfs "(call :ccos.discovery.servers {})" \
  --output-format json \
  --config ../config/agent_config.toml
```

Output:
```json
[
  {
    "name": "github",
    "endpoint": "https://api.githubcopilot.com/mcp/"
  }
]
```

## Scripting with `--rtfs-file`

Create a file `discover.rtfs`:

```rtfs
(do
  (call :ccos.discovery.discover {:server "github"})
  (call :mcp.github.list_issues {
    :owner "mandubian"
    :repo "ccos"
    :state "OPEN"
    :perPage 10
  }))
```

Execute it:

```bash
cargo run --example capability_explorer -- \
  --rtfs-file discover.rtfs \
  --output-format json \
  --quiet \
  --config ../config/agent_config.toml
```

## Examples

### Get GitHub User Info

```bash
cargo run --example capability_explorer -- \
  --rtfs '(do 
    (call :ccos.discovery.discover {:server "github"}) 
    (call :mcp.github.get_me {}))' \
  --config ../config/agent_config.toml -q
```

### List Repository Issues

```bash
cargo run --example capability_explorer -- \
  --rtfs '(do 
    (call :ccos.discovery.discover {:server "github"}) 
    (call :mcp.github.list_issues {
      :owner "mandubian" 
      :repo "ccos" 
      :state "OPEN"
      :perPage 5
    }))' \
  --config ../config/agent_config.toml
```

### Search Code

```bash
cargo run --example capability_explorer -- \
  --rtfs '(do 
    (call :ccos.discovery.discover {:server "github"}) 
    (call :mcp.github.search_code {
      :query "language:rust CapabilityMarketplace"
      :perPage 5
    }))' \
  --config ../config/agent_config.toml
```

### Get File Contents

```bash
cargo run --example capability_explorer -- \
  --rtfs '(do 
    (call :ccos.discovery.discover {:server "github"}) 
    (call :mcp.github.get_file_contents {
      :owner "mandubian" 
      :repo "ccos"
      :path "README.md"
    }))' \
  --config ../config/agent_config.toml
```

## Interactive TUI Mode

Without `--rtfs` or `--rtfs-file`, the explorer runs in interactive TUI mode:

```bash
cargo run --example capability_explorer -- --config ../config/agent_config.toml
```

Commands in TUI:
- `[1] servers` - List available servers
- `[2] discover` - Discover from a server
- `[3] search` - Search capabilities
- `[4] list` - List discovered capabilities
- `[5] inspect` - Inspect capability details
- `[6] call` - Call a capability interactively
- `[7] stats` - Show catalog statistics
- `[h] help` - Show help
- `[q] quit` - Exit

## Configuration

The explorer requires an agent configuration file. See `config/agent_config.toml` for the full configuration format.

Key sections for MCP discovery:

```toml
[discovery]
enabled = true

[[discovery.mcp_servers]]
name = "github"
endpoint = "https://api.githubcopilot.com/mcp/"
auth_token_env = "GITHUB_TOKEN"
```

## Troubleshooting

### "Invalid session ID" Error

This usually means the MCP session wasn't properly initialized. Ensure:
1. Your auth token is valid and set in the environment
2. The MCP server endpoint is accessible
3. You're using a recent version with the session ID header fix

### Type Mismatch Errors

If you see errors like "expected :float, got integer", the type coercion should handle most cases. If not, try using float literals:

```rtfs
{:perPage 5.0}  ;; Instead of {:perPage 5}
```

### Auth Token Not Working

Ensure the environment variable is set:

```bash
export GITHUB_TOKEN="your-token-here"
```

Or check the `auth_token_env` setting in your config matches the variable name.
