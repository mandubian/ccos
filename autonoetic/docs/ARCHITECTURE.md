# Autonoetic Architecture

> Autonoetic: from Greek *autonoetikos* (αὐτονοητικός) — "self-aware, having insight into one's own mental processes."

Autonoetic is a Rust-first runtime for autonomous, self-evolving AI agents with formal governance. It replaces the heavier CCOS architecture with a leaner design that separates reasoning from execution.

## Table of Contents

- [Core Design Principle](#core-design-principle)
- [System Components](#system-components)
- [Data Flow](#data-flow)
- [Security Model](#security-model)
- [Execution Modes](#execution-modes)
- [Memory Architecture](#memory-architecture)
- [Content Storage](#content-storage)
- [Causal Chain](#causal-chain)
- [Design Principles](#design-principles)

---

## Core Design Principle

**Separation of Powers**: Agents are pure reasoners. The gateway is the sole authority for execution.

```
┌─────────────────────────────────────────────────────────┐
│                     Agent (Low Privilege)                │
│                                                         │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   │
│  │  Reasoning  │ → │  Proposals  │ → │   Review    │   │
│  │   (LLM)     │   │  (Intents)  │   │  (Results)  │   │
│  └─────────────┘   └─────────────┘   └─────────────┘   │
│         │                │                   │          │
│         └────────────────┼───────────────────┘          │
│                          ▼                              │
│              Intent / Proposal Verbs:                   │
│         execute, spawn, share, schedule, recall         │
└──────────────────────────┬──────────────────────────────┘
                           │ JSON-RPC / HTTP
                           ▼
┌─────────────────────────────────────────────────────────┐
│                   Gateway (High Privilege)               │
│                                                         │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐  │
│  │ Policy  │  │Execution │  │  Audit   │  │  Secret │  │
│  │ Engine  │→ │  Engine  │→ │  Logger  │  │  Store  │  │
│  └─────────┘  └──────────┘  └──────────┘  └─────────┘  │
│         │              │              │              │   │
│         ▼              ▼              ▼              ▼   │
│  Capability      Sandbox        Causal          Vault   │
│  Validation     Execution       Chain         Injection  │
└─────────────────────────────────────────────────────────┘
```

**Why this matters:**
- Agents cannot access secrets, filesystems, or networks directly
- Gateway validates every proposal against capabilities and policy
- All execution is logged to an immutable audit trail
- Agents can be replaced; governance is permanent

## System Components

### Gateway

The gateway is the security boundary and execution engine. It is NOT a rule engine — it does not contain domain-specific business logic.

| Component | Responsibility |
|-----------|---------------|
| **JSON-RPC Router** | Accepts `event.ingest` and `agent.spawn` requests |
| **Policy Engine** | Validates capabilities, ACLs, and disclosure rules |
| **Execution Service** | Spawns agent sessions, manages lifecycle |
| **Content Store** | SHA-256 content-addressable storage for artifacts |
| **Causal Chain** | Append-only JSONL audit log with hash-chain integrity |
| **Scheduler** | Manages background reevaluation cadence and wake predicates |
| **Sandbox Runner** | Executes scripts via bubblewrap, docker, or microvm |
| **Secret Vault** | Injects secrets ephemerally, never exposes to agents |
| **HTTP API** | REST endpoints for remote agent content access |

### Agent

An agent is a SKILL.md manifest + instructions that runs inside a sandbox. Agents propose actions; the gateway executes them.

Key characteristics:
- **Pure reasoner**: Makes decisions, but cannot execute
- **Text-native**: Agent and workflow state are plain text/JSON files, prioritizing transparency over database opacity.
  - *Note:* The Gateway uses an embedded SQLite database (`gateway.db`) purely for fast-moving transactional data: approvals, notifications, and workflow events.
- **Capability-declared**: All permissions declared in manifest
- **Role-based**: Each agent fills a specific role in the system

### SDK

The `autonoetic_sdk` package (Python/TypeScript) provides the agent's view of the gateway:

| Transport | Use Case |
|-----------|----------|
| Unix socket | Local agents (same machine as gateway) |
| HTTP/REST | Remote agents (different machine) |

---

## Data Flow

### Standard Request Flow

```
1. User message arrives via JSON-RPC or HTTP
2. Gateway resolves target agent (explicit → session lead → default lead)
3. Gateway spawns agent session with context
4. Agent reasoning loop:
   a. LLM processes context + instructions
   b. LLM emits tool calls (content.write, agent.spawn, etc.)
   c. Gateway validates and executes tools
   d. Tool results returned to LLM
   e. Loop until EndTurn
5. Agent response returned through ingress channel
6. All actions logged to causal chain
```

### Content Storage Flow

```
Agent: content.write("main.py", script_content)
  ↓
Gateway: 1. Compute SHA-256 hash
         2. Store blob at .gateway/content/sha256/ab/c123...
         3. Update session manifest: {"main.py": "sha256:abc123"}
         4. Return handle to agent

Agent: content.read("main.py")
  ↓
Gateway: 1. Resolve name → handle from session manifest
         2. Fetch blob from content store
         3. Return content
```

### Artifact Creation Flow

```
1. Coder writes files via content.write()
2. Coder writes SKILL.md with YAML frontmatter
3. Gateway detects SKILL.md, extracts metadata
4. On agent.spawn completion:
   - All session content bundled into artifact
   - Artifact metadata from SKILL.md frontmatter
   - Structured artifact added to spawn response
5. Planner receives artifacts in spawn response
6. Specialized_builder reads artifacts via content.read()
```

---

## Security Model

### Capability-Based Access Control

All external interactions require declared capabilities:

| Capability | Grants Access To |
|------------|-----------------|
| `ReadAccess` | Reading agent state files |
| `WriteAccess` | Writing agent state files |
| `SandboxFunctions` | Invoking MCP and native tools |
| `CodeExecution` | Running shell commands in sandbox |
| `AgentSpawn` | Spawning child agents |
| `AgentMessage` | Messaging other agents |
| `NetworkAccess` | Network access (with host allowlist) |
| `BackgroundReevaluation` | Scheduled background wakes |

### Capability Scoping

Capabilities use pattern-based scoping:

```yaml
capabilities:
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]  # Can only write to own dir and skills
  - type: "NetworkAccess"
    hosts: ["api.open-meteo.com"]   # Can only reach specific hosts
```

### Secret Injection

Secrets are never exposed to agents directly:
1. Agent requests secret via `secrets.get("api_key")`
2. Gateway validates agent has access to requested secret
3. Gateway injects secret as environment variable for sandbox execution
4. Secret is zeroized after execution

### Disclosure Policy

Reply governance controls what the agent can tell the user:

| Class | Behavior | Example |
|-------|----------|---------|
| `public` | Verbatim | Public API responses |
| `internal` | Summary only | Internal state, session context |
| `confidential` | Redacted | Memory contents, tool outputs |
| `secret` | Never disclosed | Vault secrets, API keys |

---

## Execution Modes

### Reasoning Mode (Default)

Full LLM-driven loop for tasks requiring judgment:

```
Context → LLM → Tool Calls → Execute → Results → LLM → ... → Response
```

- Uses configured LLM provider/model
- Iterates until EndTurn or loop limit
- Supports all tool types
- Higher latency (~2s per turn), higher cost

### Script Mode (Deterministic)

Direct sandbox execution, no LLM:

```
Input → Script → Output → Return
```

- Executes declared script directly in sandbox
- No LLM call, no iteration
- Fast (~100ms), free, deterministic
- For API calls, data transforms, lookups

**Decision guide:**
| Task Type | Mode | Reason |
|-----------|------|--------|
| API calls (weather, stocks) | `script` | Deterministic, fast |
| Data transforms | `script` | No ambiguity |
| Code review | `reasoning` | Needs judgment |
| Research | `reasoning` | Requires synthesis |

---

## Memory Architecture

### Tier 1: Working Memory (Content Storage)

Agent-local files for per-tick determinism:

```
.agent_dir/
├── state/           # Checkpoint files (task.md, scratchpad.md, handoff.md)
├── history/         # Causal chain logs
└── skills/          # Installed skills
```

**Tools:** `content.write`, `content.read`, `artifact.build`, `artifact.inspect`

Content uses root-session visibility. Default is `session` (collaborative within root). Use `visibility: "private"` for scratch work. Artifacts are the mandatory boundary for review/install/execution.

### Tier 2: Durable Memory (Knowledge)

Gateway-managed facts with provenance:

**Tools:** `knowledge.store`, `knowledge.recall`, `knowledge.search`, `knowledge.share`

| Field | Description |
|-------|-------------|
| `memory_id` | Unique identifier |
| `scope` | Namespace for ACL enforcement |
| `owner_agent_id` | Agent that owns this fact |
| `writer_agent_id` | Agent that wrote this fact |
| `source_ref` | Session/turn reference for traceability |
| `content` | The actual fact |
| `content_hash` | SHA-256 for integrity |
| `visibility` | private, shared, or global |

---

## Content Storage

Content-addressable storage that works locally and remotely:

```
.gateway/
├── content/sha256/ab/c123...   # Immutable content blobs
├── sessions/<session_id>/
│   ├── manifest.json            # name → handle mappings
│   └── artifacts.json           # Artifact metadata
└── knowledge.db                 # Tier 2 durable facts
```

### Key Properties

- **Content-addressed**: SHA-256 handles, natural deduplication
- **Session-scoped**: Files named within a session with visibility control
- **Cross-session**: `session` visibility makes content visible under same root
- **Cross-agent**: Siblings see each other's session-visible content
- **Remote-accessible**: HTTP API for distributed agents

### Remote Agents

Remote agents use the HTTP Content API instead of Unix sockets:

```
┌──────────────┐    HTTP/REST    ┌──────────────┐
│ Remote Agent │ ◄─────────────► │   Gateway    │
│              │  Bearer token   │              │
└──────────────┘                 └──────────────┘
```

Configuration via manifest or environment:
```yaml
metadata:
  autonoetic:
    gateway_url: "http://gateway:8080"
    gateway_token: "secret"
```

---

## Causal Chain

All actions are logged to an append-only JSONL audit trail:

```
.gateway/history/causal_chain.jsonl
agent_dir/history/causal_chain.jsonl
```

### Entry Structure

```json
{
  "session_id": "session-123",
  "turn_id": "turn-abc",
  "event_seq": 42,
  "category": "tool",
  "action": "requested",
  "timestamp": "2026-03-15T10:30:00Z",
  "payload": {"tool_name": "content.write", ...},
  "entry_hash": "sha256:...",
  "prev_hash": "sha256:..."
}
```

### Key Events

| Category | Actions | Description |
|----------|---------|-------------|
| `session` | `start`, `end` | Session lifecycle |
| `llm` | `requested`, `completed` | LLM completion calls |
| `tool` | `requested`, `completed`, `failed` | Tool execution |
| `script` | `started`, `completed`, `failed` | Script agent execution |
| `gateway` | `event.ingest.requested`, `.completed` | Ingress events |
| `memory` | `history.persisted`, `session.forked` | Session checkpointing |

### Trace Commands

```bash
autonoetic trace sessions              # List active sessions
autonoetic trace show <session_id>     # View session timeline
autonoetic trace event <log_id>        # View specific entry
autonoetic trace rebuild <session_id>  # Reconstruct unified timeline
autonoetic trace follow <session_id>   # Watch live events
autonoetic trace fork <session_id>     # Fork from checkpoint
autonoetic trace history <session_id>  # View conversation history
```

---

## Design Principles

1. **Gateway as Dumb Secure Pipe**: Execute proposals, don't make decisions
2. **Agents as Pure Reasoners**: LLMs plan; gateway validates and acts
3. **Autonomy Through Composition**: Complex behavior emerges from simple primitives
4. **No Hardcoded Heuristics**: Business logic in SKILL.md, not platform code
5. **Spec-Driven, Not Code-Driven**: SKILL.md YAML frontmatter is the contract
6. **Pluggable Everything**: Sandbox drivers, LLM providers, capability handlers
7. **Immutable Audit Trail**: Every action logged, hash-chained, verifiable
8. **Content-Addressed Storage**: SHA-256 handles work locally and remotely
9. **Iterative Repair**: Errors are feedback, not failures; agents retry with corrections
10. **Two-Tier Validation**: Soft for LLMs (guidance), strict for scripts (enforcement)
