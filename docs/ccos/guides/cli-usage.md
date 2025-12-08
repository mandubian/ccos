# CCOS CLI Usage Guide

The `ccos` command-line interface provides unified access to CCOS capabilities, MCP server discovery, approval workflows, and configuration management.

## Available Binaries

| Binary | Package | Description |
|--------|---------|-------------|
| `ccos` | ccos | Unified CLI for capability management |
| `rtfs-repl` | rtfs | Interactive RTFS REPL |
| `rtfs-compiler` | rtfs | RTFS file compiler |

## Installation

Build from the repository root:

```bash
# Build all binaries
cargo build --release

# Or build specific binaries
cargo build --release --bin ccos
cargo build --release --bin rtfs-repl --features repl
cargo build --release --bin rtfs-compiler
```

Binaries will be available in `target/release/`.

## Quick Start

```bash
# Show help
ccos --help

# Show configuration
ccos config show

# Validate configuration
ccos config validate

# Discover capabilities from a goal
ccos discover goal "I need to search GitHub repositories"

# List discovered capabilities
ccos discover list

# Start interactive RTFS REPL
cargo run --bin rtfs-repl
```

## Global Options

All commands support these global options:

| Option | Short | Description |
|--------|-------|-------------|
| `--config <FILE>` | `-c` | Path to configuration file (default: auto-detect) |
| `--output-format <FORMAT>` | `-o` | Output format: `table`, `json`, `rtfs`, `plain` |
| `--quiet` | `-q` | Suppress status messages |
| `--verbose` | `-v` | Enable verbose/debug output |
| `--help` | `-h` | Print help |
| `--version` | `-V` | Print version |

## Commands

### config - Configuration Management

Manage agent configuration files.

```bash
# Show full configuration
ccos config show

# Show specific section
ccos config show --section llm_profiles
ccos config show --section governance
ccos config show --section discovery

# Output as JSON
ccos config show -o json

# Validate configuration
ccos config validate

# Initialize new configuration file
ccos config init
ccos config init --output my_config.toml
ccos config init --output my_config.toml --force  # Overwrite existing
```

**Available sections:** `agent`, `llm_profiles`, `discovery`, `governance`, `capabilities`, `marketplace`

### discover - Capability Discovery

Discover and explore capabilities from various sources.

```bash
# Discover from a natural language goal
ccos discover goal "I need to create GitHub issues"
ccos discover goal "Search for weather data" --sources local,registry

# Discover from a specific MCP server
ccos discover server github-mcp
ccos discover server my-server --endpoint http://localhost:3000

# Search discovered capabilities
ccos discover search "github"
ccos discover search "file" --domain filesystem

# List all discovered capabilities
ccos discover list
ccos discover list --source mcp
ccos discover list --status healthy

# Inspect a specific capability
ccos discover inspect github.create_issue
ccos discover inspect weather.get_forecast -o json
```

**Discovery sources:** `local`, `aliases`, `registry`, `apisguru`, `web`

### server - MCP Server Management

Manage MCP server connections.

```bash
# List configured servers
ccos server list

# Add a new server
ccos server add https://my-mcp-server.com/
ccos server add https://server.com/ --name my-server

# Remove a server
ccos server remove my-server

# Check server health
ccos server health my-server
ccos server health --all
```

### approval - Approval Queue Management

Manage the approval queue for external server discoveries.

```bash
# List pending approvals
ccos approval list
ccos approval list --status pending
ccos approval list --status approved
ccos approval list --status rejected

# Approve a server
ccos approval approve <approval-id>
ccos approval approve <approval-id> --reason "Verified source"

# Reject a server
ccos approval reject <approval-id>
ccos approval reject <approval-id> --reason "Untrusted source"

# Show approval details
ccos approval show <approval-id>
```

### call - Execute Capabilities

Execute a capability directly from the command line.

```bash
# Call a capability with arguments
ccos call github.search_repositories --query "rust mcp"
ccos call weather.get_forecast --location "Paris"

# Pass arguments as JSON
ccos call my.capability --args '{"key": "value"}'

# Dry run (show what would be executed)
ccos call github.create_issue --dry-run --title "Test"
```

### plan - Planning and archives

Generate, inspect, execute, and manage RTFS plans.

```bash
# Create a plan from a goal (archives to storage)
ccos plan create "Send weekly status email" --save plan.rtfs

# List archived plans (matches id, name, or goal)
ccos plan list
ccos plan list --filter email

# Execute a plan by ID, name hint, path, or raw RTFS
ccos plan execute plan-1234
ccos plan execute "Send weekly status email"

# Validate syntax and capability availability
ccos plan validate plan-1234

# Delete an archived plan
ccos plan delete plan-1234
```

### rtfs - RTFS Operations

Evaluate and run RTFS code.

```bash
# Evaluate an RTFS expression
ccos rtfs eval '(+ 1 2 3)'

# Run an RTFS file
ccos rtfs run my_script.rtfs
ccos rtfs run my_script.rtfs --show-timing
```

> **Note:** For interactive RTFS development, use the dedicated `rtfs-repl` binary instead (see below).

### explore - Interactive Explorer

Launch the interactive capability explorer (TUI mode).

```bash
# Start interactive explorer
ccos explore

# Start in specific mode
ccos explore --mode discovery
ccos explore --mode rtfs
```

## RTFS REPL

The `rtfs-repl` binary provides an interactive REPL for RTFS development:

```bash
# Start interactive REPL
cargo run --bin rtfs-repl

# Or if built
./target/release/rtfs-repl
```

### REPL Modes

```bash
# Interactive mode (default)
rtfs-repl

# Evaluate a string
rtfs-repl --input string --string '(+ 1 2 3)'

# Run a file
rtfs-repl --input file --file mycode.rtfs

# Pipe input
echo '(+ 1 2)' | rtfs-repl --input pipe

# Verbose mode
rtfs-repl --input file --file mycode.rtfs --verbose
```

### REPL Commands

Inside the interactive REPL:

| Command | Description |
|---------|-------------|
| `:help` | Show help |
| `:quit` or `:q` | Exit REPL |
| `:clear` | Clear screen |
| `:env` | Show environment |
| `:load <file>` | Load RTFS file |

See the [RTFS REPL Guide](../../rtfs-2.0/guides/repl-guide.md) for detailed documentation.

## Output Formats

### Table (default)

Human-readable formatted output with sections and bullet points.

```bash
ccos config show
```

```
Configuration
─────────────
Config file: config/agent_config.toml

Agent
─────
Agent ID: demo-agent
Profile: default
```

### JSON

Machine-readable JSON output, useful for scripting.

```bash
ccos config show -o json | jq '.agent_id'
```

### RTFS

Output as RTFS data structures.

```bash
ccos discover list -o rtfs
```

### Plain

Minimal output without formatting, for piping.

```bash
ccos discover list -o plain | grep github
```

## Configuration File

The CLI looks for configuration in these locations (in order):

1. Path specified with `--config`
2. `../config/agent_config.toml` (relative to working directory)
3. `config/agent_config.toml`
4. `agent_config.toml`

### Minimal Configuration

```toml
version = "1.0"
agent_id = "my-agent"
profile = "default"

[llm_profiles]
default = "default"

[[llm_profiles.profiles]]
name = "default"
provider = "openrouter"
model = "anthropic/claude-sonnet-4-20250514"
api_key_env = "OPENROUTER_API_KEY"

[discovery]
match_threshold = 0.65
use_embeddings = false

[governance]
enabled = true

[governance.policies.default]
risk_tier = "low"
requires_approvals = 0
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CCOS_CONFIG` | Default configuration file path |
| `OPENROUTER_API_KEY` | API key for OpenRouter LLM provider |
| `GITHUB_TOKEN` | GitHub API token for GitHub MCP |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |
| 3 | Capability not found |
| 4 | Execution error |

## Examples

### Discover and Call a Capability

```bash
# Find capabilities for a goal
ccos discover goal "search github repos"

# Inspect the capability
ccos discover inspect github.search_repositories

# Call it
ccos call github.search_repositories --query "rust cli"
```

### Approve External Server Discovery

```bash
# List pending approvals
ccos approval list --status pending

# Review the details
ccos approval show abc123

# Approve it
ccos approval approve abc123 --reason "Verified official server"
```

### JSON Pipeline

```bash
# Get capabilities as JSON and filter with jq
ccos discover list -o json | jq '.[] | select(.source == "mcp")'

# Get config section
ccos config show --section llm_profiles -o json | jq '.profiles[].name'
```

## See Also

- [CCOS Architecture](../specs/001-architecture-overview.md)
- [Capability Marketplace](../specs/004-capabilities-and-marketplace.md)
- [MCP Discovery Service](../specs/031-mcp-discovery-unified-service.md)
- [CLI Design Document](../../drafts/ccos-cli-unified-tool.md)
