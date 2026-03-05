# CCOS-NG: Command Line Interface (CLI)

The `ccos` CLI is the primary human-to-system interface for CCOS-NG. It manages the full lifecycle of the Gateway daemon, local Agents, and external interoperability.

## 1. Global Flags & Configuration

Commands can be modified by standard global flags:
- `--config <path>`: Path to a custom `config.yaml` or `policy.yaml` (default: `~/.ccos/`)
- `--log-level <level>`: Overrides the Gateway log level (`trace`, `debug`, `info`, `warn`, `error`)
- `--non-interactive`: Disables all prompts (essential for CI/CD)

---

## 2. Gateway Lifecycle (`ccos gateway`)

Commands to manage the core Rust daemon that routes messages and enforces Sandbox policies.

### `ccos gateway start`
Starts the Gateway daemon in the foreground.
- **Flags:**
  - `-d, --daemon`: Run in the background.
  - `--port <number>`: Override the default HTTP/TCP ports.
  - `--tls`: Force TLS wrapping on the OFP federation port.
- **Operation:**
  1. Reads `~/.ccos/policy.yaml` to establish global security constraints.
  2. Binds the local Unix Socket (or TCP loopback) for Agent IPC.
  3. Binds the OFP port (e.g., `4200`) for cluster federation.

### `ccos gateway stop`
Gracefully terminates a background Gateway daemon.
- **Operation:** Sends a graceful shutdown signal. The Gateway will send an OFP `Advertise` with an empty Agent list to connected peers before completely exiting.

### `ccos gateway status`
Outputs a table of Gateway health, loaded policies, active memory usage, and connected peers.

---

## 3. Agent Management (`ccos agent`)

Commands for scaffolding and orchestrating AI Agents.

### `ccos agent init <agent_id>`
Scaffolds a new CCOS-NG Agent directory.
- **Flags:**
  - `--template <name>`: E.g., `researcher`, `coder`, `auditor`.
- **Operation:**
  Creates the necessary directory structure:
  ```text
  <agent_id>/
  ├── SKILL.md
  ├── state/
  ├── skills/
  └── history/
  ```

### `ccos agent run <agent_id> [message]`
Boots an Agent and connects it to the Gateway.
- **Flags:**
  - `--interactive, -i`: Drops the user into a persistent chat loop with the Agent via `stdio`.
  - `--headless`: Boots the agent to listen for messages via the Gateway but does not attach `stdio`.
- **Operation:**
  If a `[message]` is provided, it sends that message as the kickoff instruction. If the `SKILL.md` contains an `input_schema`, the CLI will prompt the user to fill out the required UI configuration either via terminal prompts or by opening a local browser window.

### `ccos agent list`
Lists all local Agents registered with the Gateway, showing their status (Stopped, Running, Hibernating).

---

## 4. Ecosystem & Skills (`ccos skill`)

Commands for managing the tools and capabilities an Agent relies on.

### `ccos skill install <github_url_or_skill_id> [--agent <agent_id>]`
Downloads and installs an AgentSkills.io compliant bundle.
- **Operation:** Extracts the `SKILL.md` and `scripts/` sidecar into the target Agent's `skills/` directory.

### `ccos skill uninstall <skill_name> --agent <agent_id>`
Removes a skill from an Agent's capability list.

---

## 5. Federation & Cluster (`ccos federate`)

Commands exposing the OFP protocol to humans.

### `ccos federate join <peer_address>`
Connects the local Gateway to a remote Gateway (or OpenFang node).
- **Operation:** Initiates the HMAC-SHA256 handshake over TCP. Logs the negotiation of extensions (TLS, `msg_hmac`, etc.).

### `ccos federate list`
Outputs the local `PeerRegistry`, showing connected Gateways and the Remote Agents they are advertising.

---

## 6. MCP Integration (`ccos mcp`)

Commands for managing Model Context Protocol external servers.

### `ccos mcp add <server_name> --command <cmd> [args...]`
Registers a local MCP server with the Gateway.
- **Example:** `ccos mcp add github --command npx -- -y @modelcontextprotocol/server-github`
- **Operation:** The Gateway will automatically spawn this subprocess, run `tools/list`, and namespace the tools as `mcp_github_*` for Agents to use.

### `ccos mcp expose <agent_id>`
Temporarily runs the Gateway as an MCP Server on `stdio`, specifically exposing `<agent_id>` as a callable tool. This is the command used to plug a CCOS-NG Agent into Cursor, VS Code, or Claude Desktop.
- **Example Usage in Cursor Config:**
  `{"command": "ccos", "args": ["mcp", "expose", "agent_coder_alpha"]}`
