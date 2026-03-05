# CCOS-NG: System Protocols

This document defines the core data formats and communication standards governing interactions between the fundamental architectural components of CCOS-NG.

## 1. Context & Motivation

CCOS-NG abandons bespoke RPC mechanisms and thick client architectures inside Sandboxes and Agents. Instead, to maximize security and portability, all components interact using JSON-based message passing over standard I/O pipes (stdin/stdout) or simple local Unix sockets.

## 2. Gateway <-> Agent Protocol

The Gateway manages the Agent ecosystem. Communication to the actual Agent instance (whether a specialized LLM application or a wrapped third-party CLI like `aider`) requires standardizing a few distinct events.

### The Message Envelope

Every payload crossing boundaries is a serialized JSON object wrapped in a standard event envelope. 
For example:

```json
{
  "jsonrpc": "2.0",
  "method": "ecosystem.message_received",
  "params": {
    "sender_id": "human_12347",
    "channel": "whatsapp",
    "payload": {
       "type": "text",
       "content": "Can you analyze the logs roughly every hour?",
       "attachments": [
         {"type": "image", "url": "/sandbox/tmp/photo.jpg", "caption": "The screenshot is here"}
       ]
    }
  },
  "id": "req-99z240f"
}
```

### Core Methods

#### 1. Inbound to Agent (from Gateway)
* `ecosystem.message_received`: A new direct message from a Human or Sub-Agent. Includes text or multi-modal payloads (`image`, `audio`, `file`).
* `ecosystem.skill_completed`: Carries the output artifacts and exit code of an asynchronously delegated Sandbox run.
* `ecosystem.approval_granted`: Carries the granted permissions from a human or policy engine in response to an `ApprovalReq`.
* `system.shutdown_signal`: A graceful shutdown request (Hibernate), prompting the agent to serialize memory into `task.md` and exit its loop.
* `system.terminate_agent`: A forced termination (Death & GC). The agent has exceeded its TTL or completed its overarching objective. Memory is flushed, and the Gateway marks the Manifest for archiving.

#### 2. Outbound from Agent (to Gateway)
* `ecosystem.send_message`: Routes a text/multimedia payload to another entity (Sub-Agent or Human) via the message bus.
* `ecosystem.spawn_agent`: Requests the manifestation of a new subordinate Agent loop, returning a new `agent_id`.
  - **External CLI Wrapping**: The payload can specify wrapping a third-party CLI tool (e.g., `aider`, `claude-code`) instead of a generic LLM. The Gateway automatically sandboxes the process, piping the parent Agent's instructions to `stdin` and routing the tool's output back to the ecosystem.
* `ecosystem.schedule_cron`: Requests the Gateway to invoke a specific Sandbox Skill at a regular interval (The Cold Path) without waking the LLM up unless an error occurs.
* `ecosystem.skill_describe`: Requests the Gateway to load the full body of a specific `SKILL.md` file into the agent's context window. (Discovery is done upfront via the YAML frontmatter).
* `ecosystem.sandbox_execute`: Requests the asynchronous/synchronous execution of arbitrary code or a specific tool script residing in a Skill's `scripts/` sidecar directory. Because a single Skill can offer multiple tools, this payload contains the specific tool/script name alongside the `stdin` payload.
* `ecosystem.request_approval`: Suspends the current execution thread, lifting metadata about an attempted boundary violation up to the parent or Human to manually authorize.

## 3. Agent <-> Sandbox Protocol (The SDK Layer)

A Sandbox process executing a generated Python text script does not speak directly to the Agent. All SDK calls (`ccos_sdk.memory.read()`) actually flow to the Gateway as JSON-RPC requests over a local Unix socket mounted in the Sandbox.

### The Sandbox Event Stream

Sandboxes are ephemeral, stateless tasks. While running, they only emit and consume synchronous RPC events.

#### Methods (Sandbox to Gateway)
* `sdk.secret.get(name)`: Asks the Gateway to fetch an API Key. If the Policy Engine rejects it, the Gateway returns an immediate RPC error.
* `sdk.files.upload(path)` / `sdk.files.download(url)`: Requests the Gateway's networking stack to handle the actual bytes.
* `sdk.emit_log(level, string)`: Pushes standard output logs directly to the Gateway's Observability stream without waiting on the parent Agent.

#### Terminal Artifact Emission
When a Sandbox script attempts to `sys.exit()`, the Gateway intercepts the final data points before destroying the MicroVM or bwrap instance:
1. `stdout` buffer.
2. `stderr` buffer.
3. Specially designated return schemas written to `.ccos/out.json`.
The Gateway then packs these together into an `ecosystem.skill_completed` method and routes it back to the parent Agent.

## 4. External Ecosystem Protocols (MCP)

CCOS-NG natively adopts the **Model Context Protocol (MCP)** to ensure maximum interoperability with the broader AI ecosystem, rather than relying on proprietary integrations or A2A (Agent-to-Agent) complexity.

### MCP Client (Gateway acting as Client)
The Gateway can connect to external MCP servers via `stdio` (subprocess) or SSE (Server-Sent Events).
- **Tool Discovery**: Upon connection, the Gateway issues a `tools/list` request to the MCP server.
- **Namespacing**: Discovered tools are dynamically registered to all authorized Agents with the prefix `mcp_{server_name}_{tool_name}` to prevent collisions.
- **Execution**: When an Agent calls an MCP tool, the Gateway routes the JSON-RPC `tools/call` over the respective transport.

### MCP Server (Gateway acting as Server)
The Gateway exposes CCOS-NG Agents as callable MCP tools to external clients (e.g., Cursor, VS Code, Claude Desktop) via `stdio` or HTTP `POST /mcp`.
- **Tool Exposure**: Each Agent becomes a tool named `ccos_agent_{name}` accepting a `message` parameter.
- **Execution**: External IDEs can route complex tasks directly to specialized CCOS-NG agents.

## 5. Gateway-to-Gateway Federation Protocol (OFP)

CCOS-NG **natively adopts the OpenFang Wire Protocol (OFP)** as its federation layer, ensuring wire-level compatibility with OpenFang nodes. CCOS-NG Gateways can federate with each other *and* with OpenFang instances out of the box.

### Why OFP (Not a Custom Protocol)
OFP is MIT/Apache-2.0 licensed, battle-tested, and shares the same design philosophy as CCOS-NG: lightweight JSON-RPC over TCP with HMAC-SHA256 mutual authentication. Rather than reinventing an identical protocol, CCOS-NG adopts it directly and extends it with optional enhancements negotiated during handshake.

### OFP Base Protocol (Wire-Compatible with [OpenFang](https://github.com/RightNow-AI/openfang?tab=readme-ov-file))
Gateways communicate over long-lived TCP sockets using newline-delimited JSON-RPC framing.
- **Mutual Authentication**: Both Gateways execute an HMAC-SHA256 challenge-response handshake using a pre-shared cryptographic secret (`{nonce, node_id, hmac}`). Both sides challenge each other using constant-time HMAC comparison.
- **Capability Gating**: Federation requires specific capabilities (`OfpDiscover`, `OfpConnect(addr)`, `OfpAdvertise`).
- **Core Methods**:
  - `Discover` / `DiscoverResponse`: Request/receive peer Node ID and list of public Agents.
  - `Advertise`: Announce local Agents to a connected peer.
  - `RouteMessage` / `RouteResponse`: Forward a message to a remote Agent and receive the reply.
  - `Ping` / `Pong`: Keepalive heartbeats (default: 30s).

### CCOS-NG Extensions (Negotiated at Handshake)
During the HMAC handshake, CCOS-NG Gateways advertise optional extensions via an `"extensions"` field. If the peer supports them, they are activated for the session. If the peer is a vanilla OpenFang node, the extensions are simply skipped and the base OFP protocol is used.

- **`tls`**: Wraps the TCP connection in TLS (via `rustls`) for transport encryption. HMAC remains the identity mechanism. Essential for internet-facing federations; optional on trusted LANs.
- **`msg_hmac`**: Every `WireMessage` carries an HMAC-SHA256 signature over its JSON payload + a monotonic sequence number. Provides per-message integrity, replay prevention, and session hijack protection on long-lived connections.
- **`resilience`**: Enables exponential backoff reconnection (1s → 60s max), local message queuing (configurable cap, default: 100), stale peer eviction (default: 5 min timeout), and graceful shutdown (empty `Advertise` before disconnect).

### PeerRegistry
Each Gateway maintains a local `PeerRegistry` tracking all connected peers and their advertised Agents:
- `{peer_id, address, agents[], last_seen, connection_state}`
- When an Agent calls `ecosystem.send_message` targeting a remote Agent, the Gateway checks the PeerRegistry, finds the owning peer, and transparently routes via `RouteMessage`. The Agent never knows the target is on a different machine.

### OpenFang Interoperability 
Because CCOS-NG speaks native OFP, a CCOS-NG Gateway can be added as a peer to an existing OpenFang cluster by simply sharing the same `shared_secret` in both configs. OpenFang agents can discover and message CCOS-NG agents, and vice versa. The extensions (`tls`, `msg_hmac`, `resilience`) gracefully degrade when connected to a peer that does not support them.

## 6. The Causal Chain Logging Format

Every protocol event flowing through the Gateway is recorded into an immutable log stream. This acts as the backbone for the Asynchronous Auditor loop and human debugging.

All Causal logs are strictly formatted as line-delimited JSON (`.jsonl`) to support effortless search and programmatic parsing:

```json
{
  "timestamp": "2026-03-05T10:15:30Z",
  "actor_id": "agent_alpha_75g",
  "category": "sandbox_execution",
  "action": "sdk.secret.get",
  "target": "GITHUB_API_TOKEN",
  "status": "DENIED",
  "reason": "policy/strict_auth_required",
  "prev_hash": "a3f8c2...b7e1"
}
```

### Merkle Audit Trail (Tamper-Evidence)
Each Causal Chain entry includes a `prev_hash` field containing the SHA-256 hash of the previous entry. This forms a Merkle hash chain, providing cryptographic tamper-evidence:
- If any historical log entry is altered or deleted, the chain breaks and the discrepancy is immediately detectable.
- The Auditor Agent uses this chain to verify the integrity of the audit trail before performing its security analysis.
- The chain root hash can be periodically signed and published for non-repudiable external verification.

This ensures that the "Why" and "Who" behind every system-level component interaction is tracked and **cannot be retroactively falsified**, satisfying strict auditability requirements for autonomous agents running custom code.
