# 020 — Multi-Agent Architecture (CCOS Autonomous Agents)

## Status
**Draft** — Design specification for multi-agent coordination in CCOS.

## Context
The 015-capabilities-vs-agents spec established that agents are capabilities with `:kind :agent` metadata. This document extends that foundation to define how **multiple agents** coordinate, delegate, and communicate—enabling CCOS to run autonomous agent ecosystems.

## Goals
1. **Autonomous Agents**: Agents that live in their own CCOS runtime, auto-deploy, and stay alive
2. **Inter-Agent Communication**: Request/response between agents across processes/machines
3. **Task Delegation**: Meta-agents delegate to specialist agents
4. **Trust Propagation**: Delegation chains with attestation and governance
5. **Agent Lifecycle**: Spawn, hibernate, wake, terminate agents

---

## Core Concepts

### 1. Agent Identity

An agent has a persistent identity separate from capability execution:

```rust
struct AgentIdentity {
    agent_id: String,           // Unique identifier (e.g., "planner-agent-01")
    name: String,               // Human-readable name
    capabilities_owned: Vec<String>,  // Capabilities this agent created
    autonomy_level: u8,         // 0-4, maps to trust levels
    constraints: AgentConstraints,
    created_at: u64,
}

struct AgentConstraints {
    max_autonomy_level: u8,
    require_approval_domains: Vec<String>,  // e.g., ["finance", "pii"]
    max_concurrent_tasks: usize,
}
```

**Key insight**: Unlike capability execution (single-shot), an agent identity **persists** across invocations and accumulates state/learning.

### 2. Agent Addressing

Agents are addressable via a URI scheme:

```
agent://<agent-id>/<capability>          # Local agent
agent://<host>:<port>/<agent-id>/<cap>   # Remote agent
```

Examples:
```
agent://planner-01/synthesize          # Local planner agent
agent://worker.local:8081/worker-01/execute  # Remote worker
```

### 3. Agent Lifecycle States

```
                   ┌─────────┐
        spawn ───▶ │ Running │ ◀──── wake
                   └────┬────┘
                        │
          hibernate     │     terminate
                ▼       ▼         ▼
           ┌─────────┐    ┌────────────┐
           │ Dormant │    │ Terminated │
           └─────────┘    └────────────┘
```

| State | Description |
|-------|-------------|
| **Running** | Agent is alive, processing requests |
| **Dormant** | State persisted, process stopped, can wake |
| **Terminated** | Cleanup complete, agent identity archived |

---

## Inter-Agent Communication

### Message Protocol

Agents communicate via **request/response messages** over the capability call mechanism:

```rtfs
;; Agent A calls Agent B
(call :agent.invoke {
  :target "agent://worker-01/process"
  :payload {:task "summarize" :data [...]}
  :timeout_ms 30000
  :correlation_id "req-12345"
})

;; Response
{:status :ok
 :result {...}
 :source "agent://worker-01"
 :correlation_id "req-12345"}
```

### Delegation Model

When a meta-agent delegates to a worker agent:

```
┌────────────────┐         ┌────────────────┐
│  Meta-Agent    │         │  Worker-Agent  │
│  (planner)     │         │  (specialist)  │
└───────┬────────┘         └───────┬────────┘
        │                          │
        │  1. delegate(task)       │
        │ ───────────────────────▶ │
        │                          │
        │                          │ 2. execute(task)
        │                          │
        │  3. result + attestation │
        │ ◀─────────────────────── │
        │                          │
```

**Delegation includes**:
- Task specification (intent, constraints, budget)
- Delegator's attestation (proves authority to delegate)
- Trust boundary (what the worker can do)

### Trust Propagation

```rust
struct DelegationChain {
    origin: AgentId,           // Root delegator
    chain: Vec<DelegationHop>,
    constraints: PropagatedConstraints,
}

struct DelegationHop {
    from: AgentId,
    to: AgentId,
    attestation: Signature,
    restrictions: Vec<String>,  // Narrowing only allowed
    timestamp: u64,
}
```

**Rule**: Delegation can only **narrow** trust, never expand. A worker agent cannot grant itself more authority than its delegator had.

---

## Agent Deployment

### Local Agent (Same Process)

```rust
// Spawn agent in same CCOS runtime
let agent = ccos.spawn_agent(AgentConfig {
    agent_id: "worker-01".into(),
    capabilities: vec!["process.*"],
    autonomy_level: 2,
});
```

### Remote Agent (Separate Process/Machine)

```rust
// Spawn agent in new CCOS process on remote host
let agent = ccos.deploy_agent(DeploymentConfig {
    agent_id: "worker-01".into(),
    target: "remote-host:8081",
    image: "ccos-worker:latest",
    resources: ResourceSpec { cpu: 2, memory_mb: 4096 },
});
```

### Service Agents (Long-Lived)

Some agents run as services, providing capabilities to other agents:

```rtfs
(capability "github.service-agent.v1"
  :description "Long-lived GitHub service agent"
  :metadata {
    :kind :agent
    :service true              ; Marks as persistent service
    :stateful true
    :endpoints ["agent://github-agent/issues"
                "agent://github-agent/repos"]
  }
  :implementation ...)
```

---

## Agent Memory Model

Each agent has isolated memory:

```
┌─────────────────────────────────────────────┐
│                 Agent-01                     │
├─────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐   │
│  │ Working Memory  │  │ Learned Patterns│   │
│  │ (recent context)│  │ (error→fix map) │   │
│  └─────────────────┘  └─────────────────┘   │
│                                              │
│  ┌─────────────────┐  ┌─────────────────┐   │
│  │  Causal Chain   │  │ Capability      │   │
│  │  (agent-local)  │  │ Ownership       │   │
│  └─────────────────┘  └─────────────────┘   │
└─────────────────────────────────────────────┘
```

**Memory isolation** prevents one agent's learned patterns from affecting another, while still allowing explicit sharing via inter-agent messaging.

---

## Proposed Capabilities

| Capability | Description |
|------------|-------------|
| `agent.spawn` | Create and start a new agent |
| `agent.invoke` | Call a capability on a remote agent |
| `agent.delegate` | Delegate task with trust chain |
| `agent.hibernate` | Persist state and stop agent |
| `agent.wake` | Resume dormant agent |
| `agent.terminate` | Stop and archive agent |
| `agent.status` | Query agent state/health |
| `agent.list` | List all registered agents |
| `agent.recall` | Query agent's working memory |
| `agent.learn` | Store learned pattern |

---

## Security Model

### Agent Authentication

Agents authenticate via signed tokens:

```rust
struct AgentToken {
    agent_id: AgentId,
    issued_by: AgentId,       // Or "system" for bootstrap
    valid_until: u64,
    capabilities: Vec<String>,
    signature: Signature,
}
```

### Capability Restrictions

When delegating, the delegator specifies what the worker can do:

```rtfs
(call :agent.delegate {
  :to "agent://worker-01"
  :task {:intent "Summarize issues"}
  :allow [:github.issues.list :llm.summarize]  ; Whitelist
  :deny [:github.issues.delete]                ; Blacklist
  :timeout_ms 60000
})
```

### Audit Trail

All inter-agent calls are logged to the CausalChain:

```rust
ActionType::AgentInvoke {
    from: AgentId,
    to: AgentId,
    capability: String,
    delegation_chain: DelegationChain,
    result: ExecutionResult,
}
```

---

## Implementation Phases

### Phase 1 (Current)
- [x] AgentIdentity + AgentRegistry with persistence
- [x] AgentMemory wrapping WorkingMemory
- [x] Basic capabilities: agent.create, list, recall, learn

### Phase 2 (Next)
- [ ] Agent addressing (`agent://` protocol)
- [ ] agent.invoke for inter-agent calls (same process)
- [ ] Delegation chain structure

### Phase 3 (Future)
- [ ] agent.spawn/hibernate/wake/terminate lifecycle
- [ ] Remote agent deployment
- [ ] Service agents (long-lived)
- [ ] Trust propagation with attestation

---

## Open Questions

1. **State persistence format**: JSONL (current) vs SQLite vs distributed KV store?
2. **Transport protocol**: gRPC, HTTP/2, or custom over TCP?
3. **Discovery**: How do agents find each other? Registry vs broadcast vs config?
4. **Failure handling**: What happens when a delegated agent crashes mid-task?
5. **Resource accounting**: How to track/limit resources across agent hierarchy?

---

## References

- 015-capabilities-vs-agents.md — Unified artifact model
- 017-checkpoint-resume.md — Checkpoint/resume mechanism (applicable to agents)
- ai-self-programming-plan.md — Overall self-programming architecture
