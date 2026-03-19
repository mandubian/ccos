# Autonoetic Modules

> Detailed reference for all Autonoetic system modules and their responsibilities.

## Table of Contents

- [Workspace Structure](#workspace-structure)
- [autonoetic-gateway](#autonoetic-gateway)
- [autonoetic-types](#autonoetic-types)
- [autonoetic-sdk](#autonoetic-sdk)
- [autonoetic (CLI)](#autonoetic-cli)
- [Agent Directory Structure](#agent-directory-structure)
- [Runtime Configuration](#runtime-configuration)
- [Session budgets](session-budget.md) — YAML limits
- [Budget management](budget-management.md) — enforcement flow, OpenRouter catalog timing

---

## Workspace Structure

```
autonoetic/
├── autonoetic/           # CLI binary (clap-based)
│   ├── src/
│   │   ├── main.rs       # Entry point
│   │   └── cli/          # Command modules
│   │       ├── common.rs # Shared arg structs
│   │       ├── gateway.rs
│   │       ├── agent.rs
│   │       ├── trace.rs
│   │       ├── chat.rs
│   │       └── mcp.rs
│   └── tests/
├── autonoetic-gateway/   # Core gateway library
│   └── src/
│       ├── server/       # JSON-RPC + HTTP servers
│       ├── runtime/      # Agent lifecycle, tools, content store
│       ├── scheduler/    # Background reevaluation
│       ├── router/       # JSON-RPC dispatch
│       ├── causal_chain/ # Audit logging + rotation
│       ├── llm/          # LLM driver abstraction
│       ├── sandbox/      # Sandbox drivers
│       └── policy.rs     # Capability validation
├── autonoetic-types/     # Shared type definitions
│   └── src/
│       ├── agent.rs      # AgentManifest, ExecutionMode
│       ├── capability.rs # Capability enum
│       ├── background.rs # BackgroundPolicy
│       └── memory.rs     # MemoryObject, provenance
├── autonoetic-sdk/       # Agent SDKs
│   ├── python/           # autonoetic_sdk package
│   └── typescript/       # TypeScript SDK
├── autonoetic-ofp/       # OpenFang Protocol federation
├── autonoetic-mcp/       # Model Context Protocol
├── agents/               # Reference agent bundles
│   ├── lead/             # Lead agents (planner.default)
│   ├── specialists/      # Specialist agents (coder, researcher, etc.)
│   ├── evolution/        # Evolution agents (specialized_builder, etc.)
│   └── examples/         # Example agents (fibonacci, weather)
└── docs/                 # Documentation
```

---

## autonoetic-gateway

The core gateway library. Handles all server, runtime, and execution logic.

### Server Layer (`src/server/`)

| File | Responsibility |
|------|---------------|
| `mod.rs` | Gateway server startup, OFP + JSON-RPC + HTTP concurrent listeners |
| `jsonrpc.rs` | JSON-RPC server for local Unix socket clients |
| `http.rs` | HTTP REST API for remote agent content access |

**HTTP Endpoints:**

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/api/content/write` | Write content to store |
| GET | `/api/content/read/{session_id}/{name_or_handle}` | Read content (path) |
| POST | `/api/content/read` | Read content (body) |
| POST | `/api/content/persist` | Mark content as persistent |
| GET | `/api/content/names?session_id=X` | List session content names |

### Runtime Layer (`src/runtime/`)

| File | Responsibility |
|------|---------------|
| `lifecycle.rs` | Agent execution loop (LLM + tool dispatch) |
| `tools.rs` | Native tool handlers (content.*, knowledge.*, agent.*, etc.) |
| `content_store.rs` | SHA-256 content-addressable storage |
| `session_snapshot.rs` | Session checkpointing and forking |
| `tool_call_processor.rs` | Tool execution, disclosure tracking |
| `parser.rs` | SKILL.md frontmatter parser |
| `repository.rs` | Agent discovery and identity validation |
| `session_tracer.rs` | Causal chain logging helpers |
| `session_budget.rs` | Config-driven per-session LLM/tool/token/wall-clock/price budgets |
| `openrouter_catalog.rs` | Cached OpenRouter Models API metadata (`context_length`, per-token pricing) for UX and `max_session_price_usd` |
| `reevaluation_state.rs` | Background state persistence |
| `memory.rs` | Tier 2 durable memory (SQLite) |
| `foundation_instructions.md` | System prompt rules injected for all agents |

### Scheduler (`src/scheduler/`)

| File | Responsibility |
|------|---------------|
| `decision.rs` | Wake predicate evaluation |
| `store.rs` | Scheduler state persistence |
| `approval.rs` | Approval queue and resolution |
| `runner.rs` | Background execution |

### Router (`src/router/`)

| Responsibility |
|---------------|
| JSON-RPC method dispatch (`event.ingest`, `agent.spawn`) |
| Session affinity and lead binding |
| Default lead resolution |
| Ingress reliability controls (concurrency, backpressure) |

### Causal Chain (`src/causal_chain/`)

| File | Responsibility |
|------|---------------|
| `causal_chain.rs` | Append-only JSONL logger with hash-chain |
| `rotation.rs` | Log segmentation and compression |
| `structured_types.rs` | Causal entry schema |

### LLM Drivers (`src/llm/`)

| File | Responsibility |
|------|---------------|
| `mod.rs` | LLM driver trait, shared types |
| `provider.rs` | Provider resolution (30+ providers) |
| `openai.rs` | OpenAI-compatible driver |
| `anthropic.rs` | Anthropic Messages API driver |
| `gemini.rs` | Google Gemini driver |

### Policy (`src/policy.rs`)

Capability validation against agent manifest. Enforces:
- Tool invocation permissions
- Memory read/write scope boundaries
- Network access host restrictions
- Agent spawning limits

---

## autonoetic-types

Shared type definitions used across all crates.

### Key Types

| Type | Location | Description |
|------|----------|-------------|
| `AgentManifest` | `agent.rs` | Full parsed SKILL.md frontmatter |
| `ExecutionMode` | `agent.rs` | `Script` or `Reasoning` |
| `RuntimeDeclaration` | `agent.rs` | Engine, versions, sandbox type |
| `LlmConfig` | `agent.rs` | Provider, model, temperature |
| `Capability` | `capability.rs` | Enum of all capability types |
| `BackgroundPolicy` | `background.rs` | Reevaluation cadence and mode |
| `DisclosurePolicy` | `disclosure.rs` | Reply governance rules |
| `MemoryObject` | `memory.rs` | Tier 2 memory record with provenance |
| `ToolError` | `tool_error.rs` | Structured tool error for repair |

---

## autonoetic-sdk

Agent SDKs for sandbox scripts.

### Python SDK (`python/autonoetic_sdk/`)

| Module | API |
|--------|-----|
| `client.py` | `init()`, `init_remote()`, `AutonoeticSdk` class |
| `errors.py` | `AutonoeticSdkError`, `PolicyViolation`, `RateLimitExceeded` |

**API Namespaces:**

| Namespace | Methods |
|-----------|---------|
| `sdk.memory` | `read(path)`, `write(path, content)`, `remember()`, `recall()`, `search()` |
| `sdk.files` | `download(url)`, `upload(path, target)`, `read(name)`, `write(name, content)`, `persist(handle)` |
| `sdk.state` | `checkpoint(data)`, `get_checkpoint()` |
| `sdk.secrets` | `get(name)` |
| `sdk.message` | `send(agent_id, payload)`, `ask(agent_id, question)` |
| `sdk.events` | `emit(type, data)` |
| `sdk.tasks` | `post()`, `claim()`, `complete()`, `list()` |
| `sdk.artifacts` | `put()`, `mount()`, `share()` |

**Transport Modes:**

| Mode | Configuration | Use Case |
|------|---------------|----------|
| Unix socket | `CCOS_SOCKET_PATH` env or `socket_path=` | Local agents |
| HTTP | `AUTONOETIC_HTTP_URL` env or `http_url=` | Remote agents |

---

## autonoetic CLI

The `autonoetic` binary provides CLI access to all gateway and agent operations.

### Command Tree

```
autonoetic
├── gateway
│   ├── start [--port N] [--config PATH]
│   ├── stop
│   ├── status [--json]
│   └── approvals
│       ├── list [--json]
│       ├── approve <request_id> [--reason TEXT]
│       └── reject <request_id> [--reason TEXT]
├── agent
│   ├── init <name> [--template quickstart|planner|script]
│   ├── run <agent_id> [--config PATH] [--interactive]
│   ├── list [--agents-dir PATH] [--json]
│   └── bootstrap [--from PATH] [--overwrite]
├── chat [--agent ID] [--session-id ID] [--channel-id ID]
├── trace
│   ├── sessions [--agent ID] [--json]
│   ├── show <session_id> [--agent ID] [--json]
│   ├── event <log_id> [--json]
│   ├── rebuild <session_id> [--json]
│   ├── follow <session_id> [--agent ID] [--json]
│   ├── fork <session_id> [--at-turn N] [--message TEXT] [--agent ID] [--interactive]
│   └── history <session_id> [--json]
├── skill
│   ├── install <path>
│   └── uninstall <name>
├── federate
│   ├── join <host:port> [--name NAME]
│   └── list [--json]
└── mcp
    ├── add <name> [--command CMD] [--args ARGS] [--transport stdio|sse]
    └── expose [--port PORT]
```

### Key CLI Workflows

**Start gateway and chat:**
```bash
autonoetic gateway start --port 8080
autonoetic chat --agent planner.default
```

**Bootstrap reference agents:**
```bash
autonoetic agent bootstrap --from ./agents/ --overwrite
```

**Inspect session traces:**
```bash
autonoetic trace sessions
autonoetic trace show <session_id>
autonoetic trace follow <session_id>
```

**Approve pending installs:**
```bash
autonoetic gateway approvals list
autonoetic gateway approvals approve <request_id>
```

---

## Agent Directory Structure

### Runtime Directory

```
.autonoetic/
├── gateway.toml                    # Gateway configuration
├── .gateway/
│   ├── content/sha256/ab/...       # Content-addressable store
│   ├── sessions/<session_id>/
│   │   ├── manifest.json           # Name → handle mappings
│   │   └── artifacts.json          # Artifact metadata
│   ├── history/causal_chain.jsonl  # Gateway-level audit log
│   ├── scheduler/                  # Background scheduler state
│   │   ├── agent_state/
│   │   └── approvals/
│   └── knowledge.db                # Tier 2 durable memory
└── agents/
    ├── planner.default/
    │   ├── SKILL.md                # Agent manifest + instructions
    │   ├── runtime.lock            # Locked dependencies
    │   ├── state/                  # Working memory
    │   ├── history/                # Agent-level causal chain
    │   ├── skills/                 # Installed child skills
    │   └── scripts/                # Agent scripts
    ├── coder.default/
    │   └── ...
    └── ...
```

### SKILL.md Structure

```yaml
---
name: "agent.id"
description: "What this agent does"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "agent.id"
      name: "Human Name"
      description: "Description"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.1
    capabilities:
      - type: "ToolInvoke"
        allowed: ["content.", "knowledge."]
      - type: "MemoryWrite"
        scopes: ["self.*", "skills/*"]
    execution_mode: "reasoning"  # or "script"
    script_entry: "main.py"      # only for script mode
    gateway_url: "http://..."    # optional, for remote agents
    gateway_token: "secret"      # optional
    io:
      accepts:
        type: object
        properties: {...}
      returns:
        type: object
        properties: {...}
    validation: "soft"           # "soft" for LLM, "strict" for scripts
---
# Agent Instructions

Markdown body with natural language instructions.
```

---

## Runtime Configuration

### Gateway Config (`gateway.toml`)

```toml
port = 8080
ofp_port = 8081
agents_dir = "/path/to/agents"
default_lead_agent_id = "planner.default"

# Reliability controls
max_concurrent_spawns = 10
max_pending_spawns_per_agent = 3

# Schema enforcement
[schema_enforcement]
mode = "deterministic"  # disabled, deterministic, or llm
audit = true

# Agent install approval
agent_install_approval_policy = "risk_based"  # always, risk_based, never
```

Session-scoped LLM/tool/token/wall-clock/optional USD caps are configured in YAML as `session_budget` (see [session-budget.md](session-budget.md)).

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AUTONOETIC_LLM_BASE_URL` | Override LLM provider URL |
| `AUTONOETIC_LLM_API_KEY` | Override LLM API key |
| `AUTONOETIC_DOCKER_IMAGE` | Docker image for sandbox |
| `AUTONOETIC_FIRECRACKER_CONFIG` | Firecracker config path |
| `AUTONOETIC_PYTHON_SDK_PATH` | Python SDK path for sandbox |
| `AUTONOETIC_SHARED_SECRET` | Bearer token for HTTP API |
| `AUTONOETIC_EVIDENCE_MODE` | `full` (default) or `off` |
| `AUTONOETIC_LLM_CONTEXT_WINDOW` | Optional token context size for CLI “% of context used” when not set in agent `llm_config` |
| `CCOS_SOCKET_PATH` | Unix socket path for SDK |

### Agent Config (`SKILL.md` frontmatter)

All agent configuration is in the `metadata.autonoetic` section of SKILL.md. See [SKILL.md Structure](#skillmd-structure) above.
