# Autonoetic CLI Reference

> Complete reference for the `autonoetic` command-line interface.

## Quick Start

```bash
# Bootstrap reference agents and start gateway
autonoetic agent bootstrap --from ./agents/ --overwrite
autonoetic gateway start --port 8080 --config gateway.toml

# Chat with an agent
autonoetic chat --agent planner.default

# Inspect traces
autonoetic trace sessions
autonoetic trace show <session_id>
```

## Global Options

| Option | Description |
|--------|-------------|
| `-c, --config <PATH>` | Path to gateway.toml config file |
| `--non-interactive` | Disable interactive prompts |

---

## Gateway Commands

### `autonoetic gateway start`

Start the gateway daemon (JSON-RPC + OFP + HTTP listeners).

```bash
autonoetic gateway start [OPTIONS]

Options:
  --port <PORT>        Gateway JSON-RPC port (default: from config)
  --config <PATH>      Path to gateway.toml
```

**Environment variables:**
- `AUTONOETIC_SHARED_SECRET` — Bearer token for HTTP API
- `AUTONOETIC_LLM_BASE_URL` — Override LLM provider URL
- `AUTONOETIC_LLM_API_KEY` — Override LLM API key

### `autonoetic gateway stop`

Stop the running gateway daemon.

### `autonoetic gateway status`

Show gateway status including connected agents, MCP servers, and scheduler state.

```bash
autonoetic gateway status [--json]
```

### `autonoetic gateway approvals`

Manage pending approval requests for `agent.install` and scheduled actions.

```bash
autonoetic gateway approvals list [--json]
autonoetic gateway approvals approve <request_id> [--reason TEXT]
autonoetic gateway approvals reject <request_id> [--reason TEXT]
```

**Approval ID format:** Short IDs like `apr-db51b7ad` (12 chars). LLMs won't truncate these.

**Auto-execute:** After approval, the gateway automatically completes the install - no agent retry needed.

---

## Agent Commands

### `autonoetic agent init`

Scaffold a new agent directory with role-specific LLM configuration.

```bash
autonoetic agent init <name> [OPTIONS]

Options:
  --template <TEMPLATE>   Template (planner, researcher, coder, auditor, generic)
  --preset <PRESET>       Named LLM preset from config (e.g., agentic, coding, fast)
  --provider <PROVIDER>   LLM provider override (openai, anthropic, gemini, openrouter)
  --model <MODEL>         LLM model override (gpt-4o, claude-sonnet-4-20250514)
```

**Examples:**

```bash
# Use template-specific default LLM (planner → claude, coder → claude)
autonoetic agent init my_coder --template coder

# Use a named preset from config
autonoetic agent init my_agent --preset coding

# Override LLM directly
autonoetic agent init my_agent --provider anthropic --model claude-sonnet-4-20250514
```

Creates:
- `SKILL.md` with manifest frontmatter and LLM config
- `runtime.lock` with dependencies
- `state/`, `history/`, `skills/`, `scripts/` directories

### `autonoetic agent presets`

List available LLM presets and template mappings.

```bash
autonoetic agent presets
```

### `autonoetic agent run`

Execute an agent directly (without gateway ingress).

```bash
autonoetic agent run <agent_id> [OPTIONS]

Options:
  --config <PATH>       Gateway config path
  --interactive         Interactive stdin chat loop
  --config FILE         Agent config for runtime
```

### `autonoetic agent list`

List all installed agents.

```bash
autonoetic agent list [--agents-dir PATH] [--json]
```

### `autonoetic agent bootstrap`

Seed reference agent bundles into the runtime agents directory.

```bash
autonoetic agent bootstrap [--from PATH] [--overwrite]
```

---

## Chat Command

Connect to an agent via terminal chat (routes through gateway `event.ingest`):

```bash
autonoetic chat [OPTIONS]

Options:
  --agent <ID>           Target agent ID (default: implicit routing)
  --session-id <ID>      Session identifier (auto-generated if omitted)
  --sender-id <ID>       Sender identifier (default: "terminal")
  --channel-id <ID>      Channel identifier (default: "terminal")
```

**Implicit routing:** If `--agent` is omitted, routes through the session's bound lead agent (or `default_lead_agent_id`).

**Session persistence:** `--session-id` enables multi-turn conversations with context retention.

**Commands during chat:**
- `/exit` or `/quit` — Exit chat
- `/status` — Show current session info

---

## Trace Commands

### `autonoetic trace sessions`

List all sessions with causal chain activity.

```bash
autonoetic trace sessions [--agent <ID>] [--json]
```

### `autonoetic trace show`

View a session's timeline of events.

```bash
autonoetic trace show <session_id> [--agent <ID>] [--json]
```

### `autonoetic trace event`

View a specific causal chain entry.

```bash
autonoetic trace event <log_id> [--json]
```

### `autonoetic trace rebuild`

Reconstruct a unified timeline from gateway + agent causal logs.

```bash
autonoetic trace rebuild <session_id> [--json]
```

### `autonoetic trace follow`

Watch session events in real-time.

```bash
autonoetic trace follow <session_id> [--agent <ID>] [--json]
```

Press Ctrl+C to stop following.

### `autonoetic trace fork`

Fork a session from a checkpoint to explore alternative paths.

```bash
autonoetic trace fork <session_id> [OPTIONS]

Options:
  --at-turn <N>         Fork from specific turn (default: latest)
  --message <TEXT>       Branch prompt (e.g., "try a different approach")
  --agent <ID>          Fork into a different agent
  --interactive         Start chat after forking
```

### `autonoetic trace history`

View the conversation history of a session.

```bash
autonoetic trace history <session_id> [--json]
```

---

## Skill Commands

### `autonoetic skill install`

Install a skill from a local directory.

```bash
autonoetic skill install <path>
```

### `autonoetic skill uninstall`

Remove an installed skill.

```bash
autonoetic skill uninstall <name>
```

---

## Federate Commands

### `autonoetic federate join`

Connect to a remote Autonoetic gateway for federation.

```bash
autonoetic federate join <host:port> [--name <NAME>]
```

### `autonoetic federate list`

List connected federation peers.

```bash
autonoetic federate list [--json]
```

---

## MCP Commands

### `autonoetic mcp add`

Register an MCP server for tool discovery.

```bash
autonoetic mcp add <name> [--command <CMD>] [--args <ARGS>] [--transport stdio|sse]
```

### `autonoetic mcp expose`

Run the gateway as an MCP server for external clients.

```bash
autonoetic mcp expose [--port <PORT>]
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |
| 3 | Agent not found |
| 4 | Permission denied |
| 5 | Network/connectivity error |
| 6 | Invalid arguments |

---

## Common Workflows

### Start Gateway and Chat

```bash
# Start gateway in background
autonoetic gateway start --port 8080 &

# Chat with planner (implicit routing)
autonoetic chat
```

### Debug a Session

```bash
# List recent sessions
autonoetic trace sessions

# View session timeline
autonoetic trace show session-abc123

# Follow live events
autonoetic trace follow session-abc123

# View specific entry
autonoetic trace event causal-log-42 --json
```

### Approve Agent Install

```bash
# List pending approvals
autonoetic gateway approvals list

# Approve a specific request
autonoetic gateway approvals approve c19a8a50-d6c8-4c5f-aa3c-6ba119751b11 \
  --reason "Weather agent looks safe"
```

### Fork and Explore

```bash
# Fork from turn 5 with alternative approach
autonoetic trace fork session-abc123 --at-turn 5 \
  --message "Try a simpler implementation" --interactive
```

### Bootstrap Reference Agents

```bash
# Create config first (required for bootstrap)
cat > gateway.toml << 'EOF'
port = 8080
agents_dir = "./agents"
default_lead_agent_id = "planner.default"
EOF

# Bootstrap all reference bundles
autonoetic agent bootstrap --from ./agents/ --overwrite

# Start and verify
autonoetic gateway start --config gateway.toml
autonoetic agent list
```
