# Autonoetic CLI Reference

The Autonoetic CLI (`autonoetic`) provides commands for managing the gateway, agents, traces, and integrations.

## Global Options

| Option | Description |
|--------|-------------|
| `-c, --config <PATH>` | Path to config file (default: `~/.ccos/config.yaml`) |
| `--non-interactive` | Disables all prompts (for CI/CD) |

## Commands

---

## Gateway

Manage the Gateway lifecycle and configuration.

### `autonoetic gateway start`

Starts the Gateway daemon.

```bash
autonoetic gateway start
autonoetic gateway start --daemon
autonoetic gateway start --port 8080
```

| Option | Description |
|--------|-------------|
| `-d, --daemon` | Run in background as daemon |
| `--port <PORT>` | Override default HTTP/TCP port |
| `--tls` | Force TLS on OFP federation port |

### `autonoetic gateway stop`

Gracefully stops a background Gateway daemon.

```bash
autonoetic gateway stop
```

### `autonoetic gateway status`

Shows Gateway health and loaded policies.

```bash
autonoetic gateway status
autonoetic gateway status --json
```

| Option | Description |
|--------|-------------|
| `--json` | Emit machine-readable JSON output |

### `autonoetic gateway approvals`

Manage background approval requests.

```bash
# List pending approvals
autonoetic gateway approvals list

# Approve a request
autonoetic gateway approvals approve <request_id> --reason "Approved"

# Reject a request  
autonoetic gateway approvals reject <request_id> --reason "Not needed"
```

---

## Agent

Manage Autonoetic agents.

### `autonoetic agent init`

Scaffolds a new agent directory.

```bash
autonoetic agent init my-agent
autonoetic agent init researcher --template researcher
```

| Argument | Description |
|----------|-------------|
| `agent_id` | Agent ID to create |

| Option | Description |
|--------|-------------|
| `-t, --template` | Template to use (researcher, coder, auditor, etc.) |

### `autonoetic agent run`

Boots an agent and connects to the Gateway.

```bash
# Run with initial message
autonoetic agent run my-agent "Hello"

# Interactive chat mode
autonoetic agent run my-agent --interactive

# Headless mode
autonoetic agent run my-agent --headless
```

| Argument | Description |
|----------|-------------|
| `agent_id` | Agent ID to run |
| `message` | Initial message (optional) |

| Option | Description |
|--------|-------------|
| `-i, --interactive` | Persistent chat loop |
| `--headless` | Boot without user interaction |

### `autonoetic agent list`

Lists all local agents registered with the Gateway.

```bash
autonoetic agent list
```

### `autonoetic agent bootstrap`

Bootstraps runtime agents from reference bundles.

```bash
autonoetic agent bootstrap
autonoetic agent bootstrap --from /path/to/bundles
autonoetic agent bootstrap --overwrite
```

| Option | Description |
|--------|-------------|
| `-f, --from` | Path to reference bundles root |
| `-o, --overwrite` | Overwrite existing agents |

---

## Chat

Chat with an agent through the Gateway JSON-RPC ingress.

```bash
# Chat with default agent
autonoetic chat

# Target specific agent
autonoetic chat researcher.default

# With specific session
autonoetic chat researcher.default --session-id my-session
```

| Argument | Description |
|----------|-------------|
| `agent_id` | Target agent ID (optional) |

| Option | Description |
|--------|-------------|
| `--sender-id` | Stable sender identity |
| `--channel-id` | Stable channel identity |
| `--session-id` | Stable conversation ID |
| `--test-mode` | Suppress prompts for scripted tests |

---

## Trace

Inspect causal chain traces for debugging and audit.

### `autonoetic trace sessions`

List known sessions across agent traces.

```bash
autonoetic trace sessions
autonoetic trace sessions --agent planner.default
autonoetic trace sessions --json
```

| Option | Description |
|--------|-------------|
| `--agent` | Restrict to specific agent |
| `--json` | Machine-readable JSON output |

### `autonoetic trace show`

Show all events for one session.

```bash
autonoetic trace show session-123
autonoetic trace show session-123 --agent planner.default
autonoetic trace show session-123 --json
```

| Argument | Description |
|----------|-------------|
| `session_id` | Session identifier |

| Option | Description |
|--------|-------------|
| `--agent` | Restrict to specific agent |
| `--json` | Machine-readable JSON output |

### `autonoetic trace event`

Show one specific event by log ID.

```bash
autonoetic trace event log-123
autonoetic trace event log-123 --agent planner.default
autonoetic trace event log-123 --json
```

| Argument | Description |
|----------|-------------|
| `log_id` | Event/log identifier |

| Option | Description |
|--------|-------------|
| `--agent` | Restrict to specific agent |
| `--json` | Machine-readable JSON output |

### `autonoetic trace rebuild`

Rebuild unified session timeline from gateway + agent causal logs.

```bash
autonoetic trace rebuild session-123
autonoetic trace rebuild session-123 --agent planner.default
autonoetic trace rebuild session-123 --json
autonoetic trace rebuild session-123 --skip-checks
```

| Argument | Description |
|----------|-------------|
| `session_id` | Session identifier |

| Option | Description |
|--------|-------------|
| `--agent` | Restrict to specific agent |
| `--json` | Machine-readable JSON output |
| `--skip-checks` | Skip integrity checks |

### `autonoetic trace follow`

Follow session events in real-time as they happen.

```bash
autonoetic trace follow session-123
autonoetic trace follow session-123 --agent planner.default
autonoetic trace follow session-123 --json
```

| Argument | Description |
|----------|-------------|
| `session_id` | Session identifier |

| Option | Description |
|--------|-------------|
| `--agent` | Restrict to specific agent |
| `--json` | Machine-readable JSON output |

Press `Ctrl+C` to stop following.

---

## Skill

Manage AgentSkills.io ecosystem and skills.

### `autonoetic skill install`

Downloads and installs an AgentSkills.io compliant bundle.

```bash
autonoetic skill install https://github.com/user/repo
autonoetic skill install my-skill --agent researcher.default
```

| Argument | Description |
|----------|-------------|
| `url_or_id` | GitHub URL or Skill ID |

| Option | Description |
|--------|-------------|
| `-a, --agent` | Target agent ID |

### `autonoetic skill uninstall`

Removes a skill from an agent's capability list.

```bash
autonoetic skill uninstall my-skill --agent researcher.default
```

| Argument | Description |
|----------|-------------|
| `skill_name` | Name of skill to uninstall |

| Option | Description |
|--------|-------------|
| `-a, --agent` | Target agent ID (required) |

---

## Federate

Manage federation and cluster connections.

### `autonoetic federate join`

Connects the local Gateway to a remote peer via OFP.

```bash
autonoetic federate join peer.example.com:9000
```

| Argument | Description |
|----------|-------------|
| `peer_address` | Remote peer address |

### `autonoetic federate list`

Outputs the local PeerRegistry.

```bash
autonoetic federate list
```

---

## MCP

Manage MCP (Model Context Protocol) integrations.

### `autonoetic mcp add`

Registers a local MCP server with the Gateway.

```bash
# Stdio transport
autonoetic mcp add my-server --command "npx"

# SSE transport
autonoetic mcp add my-server --sse-url http://localhost:3000
```

| Argument | Description |
|----------|-------------|
| `server_name` | MCP server name |

| Option | Description |
|--------|-------------|
| `-c, --command` | Subprocess command (stdio transport) |
| `--sse-url` | SSE endpoint URL |
| `args` | Command arguments (last) |

### `autonoetic mcp expose`

Runs the Gateway as an MCP Server on stdio.

```bash
autonoetic mcp expose researcher.default
```

| Argument | Description |
|----------|-------------|
| `agent_id` | Agent ID to expose |

---

## Examples

### Basic Workflow

```bash
# Start gateway
autonoetic gateway start --daemon

# Create an agent
autonoetic agent init my-researcher --template researcher

# Bootstrap reference agents
autonoetic agent bootstrap

# Chat with agent
autonoetic chat my-researcher

# Check trace
autonoetic trace sessions
autonoetic trace show session-123
```

### Background Processing

```bash
# Start gateway
autonoetic gateway start --daemon

# Check pending approvals
autonoetic gateway approvals list

# Approve a request
autonoetic gateway approvals approve req-456 --reason "Approved for execution"

# Follow session in real-time
autonoetic trace follow session-789
```

### Federation

```bash
# Join a peer
autonoetic federate join peer.example.com:9000

# List peers
autonoetic federate list
```

---

## Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error (missing config, invalid arguments, etc.) |
| 130 | Interrupted (Ctrl+C) |
