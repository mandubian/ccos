# Separation of Powers: Agents Reason, Gateway Decides

## Core Principle

**Agents are pure reasoners. They propose "what should happen." The gateway is the sole authority that decides "what actually happens."**

Every critical decision — resource allocation, secret access, inter-agent communication, scheduling, approval gates — lives in the gateway. The agent reasons, plans, delegates, and evolves, but it never touches anything sensitive or scarce directly.

This gives Autonoetic its two key properties at once: **powerful autonomous reasoning** (the agent can do anything it can propose) and **constrained execution** (the gateway enforces boundaries the agent cannot bypass).

```
Agent (low-privilege):           Gateway (high-privilege):
┌─────────────────────┐          ┌──────────────────────┐
│  READ:               │          │  MANAGES:             │
│  - SKILL.md          │          │  - Secrets/Vault      │
│  - state/task.md     │          │  - Network sockets    │
│  - skill catalog     │          │  - Filesystem writes  │
│  - memory            │          │  - Agent spawning     │
│  - causal chain      │          │  - Approval gates     │
│                      │          │  - Capability grants  │
│  PROPOSES:           │          │  - Backpressure       │
│  - "run this skill"  │          │                       │
│  - "spawn agent X"   │          │  EXECUTES:            │
│  - "share memory"    │          │  - Sandboxed scripts  │
│  - "schedule task"   │  ──→    │  - Tool invocations   │
│                      │  ←──    │  - API calls          │
│  RECEIVES:           │          │  - Resource allocation│
│  - tool results      │          │                       │
│  - structured errors │          │  AUDITS:              │
│  - memory summaries  │          │  - Causal chain       │
│                      │          │  - Policy violations  │
│  NO ACCESS TO:       │          │  - Spend tracking     │
│  - Raw secrets       │          │                       │
│  - Network directly  │          │  AUTHORIZES:          │
│  - Other agents      │          │  - Inter-agent comms  │
│  - Scheduling        │          │  - Secret injection   │
│  - Approvals         │          │  - Capability grants  │
└─────────────────────┘          └──────────────────────┘
```

---

## Delegation

**Agent proposes** which agent to spawn, when, and with what instructions.

**Gateway decides** whether it's allowed, whether resources are available, and actually creates the process.

### Agent side

The agent's `SKILL.md` declares delegation capabilities:

```yaml
metadata:
  capabilities:
    - agent.spawn
  spawn_policy:
    allowed_targets:
      - researcher.default
      - coder.default
```

The agent's reasoning loop decides when delegation is needed:

```
Goal: "Build a competitive analysis report"

Agent thinks:
  1. I need research → propose: spawn researcher.default
  2. I need code     → propose: spawn coder.default

Agent calls: gateway.agent.spawn(target="researcher.default", instructions="...")
```

### Gateway side

The gateway receives the proposal and checks every boundary:

```
Gateway decides:
  - Does this agent have agent.spawn capability?        ✓
  - Is researcher.default an allowed target?             ✓
  - Is concurrency budget available?                     ✓
  - Is target agent manifest valid?                      ✓
  → EXECUTES: spawns researcher, returns handle
```

The gateway also enforces backpressure (max concurrent spawns, per-agent queue limits) and logs the spawn to the causal chain for audit.

---

## Reevaluation

**Agent proposes** what to do when woken and how often to be woken.

**Gateway controls** the clock, deduplication, and whether the agent is allowed background wakes at all.

### Agent side

The agent's `SKILL.md` declares a reevaluation schedule:

```yaml
metadata:
  background:
    enabled: true
    schedule: "every 20 minutes"
    purpose: "check pending approvals and retry failed tasks"
```

The agent's reasoning loop handles the tick signal:

```
Gateway fires: { signal: "tick", timestamp: "..." }

Agent reads: state/reevaluation.json
Agent thinks:
  - "I have pending_approval_123 from 2 hours ago"
  - "My last scrape task failed with timeout"
  - Proposed action: gateway.approval.status("pending_approval_123")
  - Proposed action: agent.spawn(researcher.default, "retry scrape X")
```

### Gateway side

The gateway owns the scheduler. It fires `tick` signals, deduplicates overlapping wakes, respects backpressure, and logs every wake reason to the causal chain. The agent never sets timers or manages scheduling.

---

## Secrets

**Agent requests** that a secret be injected for a specific tool.

**Gateway decides** whether the agent is authorized, injects the secret as an ephemeral environment variable, and the agent never sees the value.

### The Ephemeral Injection Pattern

This is the critical security boundary. The LLM never sees raw secret values. The agent's state never contains them. The gateway is the sole custodian.

### Agent side

The agent declares which secrets its skills need:

```yaml
metadata:
  capabilities:
    - secrets.get
  declared_secrets:
    - GITHUB_TOKEN
```

The agent requests authorization:

```
Agent thinks: "I need to call the GitHub API"
Agent proposes: gateway.secrets.request("GITHUB_TOKEN", for_tool="github_search")
```

### Gateway side

```
Gateway decides:
  - Does this agent have secrets.get capability?         ✓
  - Is GITHUB_TOKEN in declared_secrets?                 ✓
  - Is tool "github_search" authorized for this secret?  ✓
  → RESULT: "approved"
```

The agent receives only a boolean approval. When the gateway executes the sandboxed skill, it injects `GITHUB_TOKEN=ghp_...` as an ephemeral env var. The secret exists only in the sandbox process memory for the duration of execution.

---

## Memory Sharing

**Agent proposes** what to share and with whom.

**Gateway enforces** ACLs, scope policies, and provenance tracking.

### Agent side

```
Agent thinks: "I want to share my research findings with the coder agent"
Agent proposes: gateway.memory.share(
  memory_id="research_findings",
  target="coder.default",
  scope="project_X"
)
```

### Gateway side

```
Gateway decides:
  - Does this agent own memory_id "research_findings"?              ✓
  - Is memory scope "project_X" sharable? (check ACLs)             ✓
  - Is "coder.default" allowed to receive this scope?              ✓
  → EXECUTES: updates ACL, returns handle to coder agent
```

The agent decides *what* to share and *with whom*. The gateway enforces *whether it's allowed* and records provenance for every access.

---

## The Vocabulary of Proposals

The agent doesn't call functions — it proposes **intent verbs** that the gateway interprets, validates, and executes:

| Verb | Agent Says | Gateway Does |
|---|---|---|
| `execute` | "Run skill X with these params" | Validates capability, spawns sandbox, injects secrets, returns result |
| `spawn` | "Create agent Y with these instructions" | Validates policy, allocates resources, starts agent |
| `share` | "Share memory Z with agent W" | Checks ACLs, updates visibility |
| `schedule` | "Wake me every N minutes" | Registers with scheduler, deduplicates |
| `recall` | "Get memory matching query Q" | Searches tier2, applies ACL filters, returns summaries |
| `request` | "I need approval for capability C" | Enqueues approval, returns status |

Every verb is a gateway-enforced boundary. The agent proposes; the gateway decides and executes.

---

## Agent Architecture

The agent is a reasoning loop with a capabilities vocabulary but no execution authority:

```
┌─────────────────────────────────────────────────┐
│                 Agent Runtime                     │
│                                                   │
│  ┌───────────┐    ┌──────────┐    ┌───────────┐  │
│  │ SKILL.md  │───→│ Reasoning │───→│ Proposals │  │
│  │ (persona  │    │   Loop    │    │ (intent   │  │
│  │  + rules) │    │           │    │  verbs)   │  │
│  └───────────┘    └────┬─────┘    └─────┬─────┘  │
│                        │                │         │
│  ┌───────────┐    ┌────▼─────┐    ┌─────▼─────┐  │
│  │  Memory   │───→│ Context  │───→│ LLM Call  │  │
│  │ (state/ + │    │ Assembly │    │ (provider │  │
│  │  tier2)   │    │          │    │  agnostic)│  │
│  └───────────┘    └──────────┘    └───────────┘  │
│                                                   │
│  CAN ONLY:              CANNOT:                   │
│  - Read skills          - Access secrets directly │
│  - Read/write state/    - Make network calls      │
│  - Read memory          - Spawn processes         │
│  - Propose actions      - Set schedules           │
│  - Request capabilities - Share memory directly   │
│  - Receive results      - Bypass approval gates   │
└─────────────────────────────────────────────────┘
                        │
                        │ Proposals (JSON-RPC)
                        ▼
┌─────────────────────────────────────────────────┐
│                  Gateway                          │
│                                                   │
│  Receives proposals → Checks policy → Executes   │
│  Enforces capability boundaries                  │
│  Manages all resources and secrets               │
│  Logs everything to causal chain                 │
└─────────────────────────────────────────────────┘
```

---

## Why This Works

**The agent is powerful but constrained.** It can reason, plan, delegate, and evolve — but it can't leak secrets, spam resources, or bypass policy. The LLM has maximum reasoning freedom within an execution cage it cannot escape.

**The gateway is simple but authoritative.** It doesn't understand delegation patterns, reevaluation heuristics, or knowledge evolution workflows. It just validates proposals against policy and executes them. This keeps it generic: completely different agent architectures can use the same gateway with different composition patterns — swarms, consensus-based delegation, ML-driven reevaluation — without changing a line of gateway code.

**The autonoetic properties emerge from agent composition of gateway primitives**, not from the gateway hardcoding orchestration patterns. A planner agent composes `agent.spawn` + `task.board` into delegation. A background agent composes `scheduler.interval` + `agent.state` into reevaluation. A coder agent composes `skill.store` + `approval.queue` into evolution.

The gateway "ensures" autonoetic behavior by making it **possible and auditable**, not by making it **prescriptive**.
