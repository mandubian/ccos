# Autonoetic Architecture Summary

*How to make autonomous agents powerful yet simple, modular, and generic — without bloat.*

---

## The Problem We're Solving

CCOS was highly ambitious but grew too large and interleaved. Autonoetic is its successor: a standalone runtime for autonomous, self-evolving agents that deliberately drops legacy complexity in favor of a thinner, more modular architecture.

The challenge: how do you build agents that are **powerful** (self-evolving, memory-bearing, multi-agent) but also **simple** (thin gateway, no hardcoded heuristics), **modular** (plug any LLM, any sandbox, any channel), and **generic** (not opinionated about specific delegation or evolution patterns)?

---

## Core Design: Separation of Powers

The answer is a strict separation between **who reasons** and **who executes**:

```
┌─────────────────────────────────────────────────────────┐
│                    AGENT (Pure Reasoner)                 │
│                                                          │
│  Reads:  SKILL.md, state/, memory, skill catalog        │
│  Thinks: Plans, delegates, evolves                      │
│  Proposes: Intent verbs to the gateway                  │
│                                                          │
│  Never touches: secrets, network, other agents,         │
│                 scheduling, filesystem                   │
└───────────────────────┬─────────────────────────────────┘
                        │  Proposals (intent verbs)
                        ▼
┌─────────────────────────────────────────────────────────┐
│                 GATEWAY (Authority)                      │
│                                                          │
│  Receives proposals → Validates policy → Executes       │
│  Sole custodian of: secrets, network, spawning,         │
│                     scheduling, approvals, memory ACLs   │
│  Everything logged to immutable causal chain             │
└─────────────────────────────────────────────────────────┘
```

**Agents are low-privilege reasoners.** They can read their skills, their state, and their memory. They can propose actions. They cannot execute anything directly.

**The gateway is the high-privilege authority.** It owns all resources, secrets, and execution. Every proposal from an agent is validated against policy before anything happens.

---

## What's Meaningful (Keep These)

### LLM Driver Abstraction

The thin driver pattern is sound. Each driver ≤250 LOC, all credential resolution centralized in `provider.rs`, three wire formats (OpenAI-compatible, Anthropic, Gemini) covering dozens of providers. The agent never knows which model it's running on.

### Gateway as Security Boundary

All I/O through a single choke point. The agent loop produces tool calls; the gateway intercepts, validates, and executes them. This is the architectural heart.

### Sandbox Driver Model

Pluggable isolation (bubblewrap, docker, microvm). The gateway passes limited, ephemeral permissions into each sandbox. Generated code runs in strict isolation.

### Capability-Based Security

Every agent declares what it needs. The gateway computes effective authority as the intersection of agent capabilities and skill declared effects. Least-privilege by default.

### Artifact Store & Causal Chain

Content-addressed storage for large data. Immutable append-only audit log with hash-chain linkage. Everything is provenance-tracked.

---

## What's Bloated (Externalize These)

The problem isn't the adapter pattern — it's the gateway accumulating domain-specific orchestration logic that should live in agent space:

### 1. Role Registry & Specialist Catalog

**Current**: Gateway has `planner.default`, `researcher`, `coder`, `debugger`, `evaluator`, `auditor`, `architect`, `memory-curator`, `evolution-steward` — 9 hardcoded role names in the routing model.

**Fix**: The gateway provides `agent.spawn(agent_id, instructions)`. The **planner agent** decides which agents to spawn based on its own SKILL.md. The gateway never knows what a "researcher" is.

### 2. Wake Predicates & Reevaluation Logic

**Current**: Gateway has predicates for stale goals, retryable failures, unresolved queued work, new inbound messages, delegated task completions, explicit timers, memory-aware reevaluation.

**Fix**: Gateway provides `scheduler.interval(agent_id, period)` and `scheduler.signal(agent_id, name, payload)`. The **agent** decides what a tick means and what to do when woken. The gateway just fires the signal.

### 3. Implicit Ingress Routing

**Current**: Gateway resolves `event.ingest` without `target_agent_id` to `default_lead_agent_id`, then the lead "chooses the best specialist."

**Fix**: Require explicit `target_agent_id` on ingress. The routing intelligence belongs in agents, not in the gateway's request parsing.

### 4. Disclosure Policy Complexity

**Current**: 7 origin categories, 4 disclosure classes, path-based defaults. Gateway must understand the *semantics* of data flow.

**Fix**: Simplify to tool-level `public`/`secret` tags. Tools declare whether their output is safe to repeat. The gateway applies a simple filter. No origin classification needed.

### 5. Textual State-Machine Conventions

**Current**: Gateway prescribes `task.md`, `scratchpad.md`, `handoff.md` with status markers.

**Fix**: Document as best practice, not enforced platform behavior. Let agents adopt conventions organically.

### 6. Evolution/Skill-Creation Pipeline

**Current**: `specialized_builder`, `evolution-steward`, approval-gated skill creation, install-time validation, PoC execution — deeply specific to one autonoetic use case.

**Fix**: Gateway provides `skill.store` and `approval.queue` primitives. The evolution narrative is agent-authored, not gateway-orchestrated.

---

## The Gateway as Dumb Secure Pipe

Strip the gateway down to its essential primitives:

| Primitive | What It Does |
|-----------|-------------|
| `agent.spawn(id, instructions)` | Start an agent, return handle |
| `skill.execute(name, params)` | Run a skill in sandbox with injected secrets |
| `skill.store.publish(bundle)` | Store a skill (pending approval) |
| `skill.store.describe(name)` | Load skill description into context |
| `memory.remember(data)` | Write to tier2 with provenance |
| `memory.recall(query)` | Search tier2 with ACL filtering |
| `memory.share(id, target, scope)` | Update ACLs for cross-agent access |
| `scheduler.interval(id, period)` | Register periodic wake signal |
| `scheduler.signal(id, name, payload)` | Fire event to agent |
| `secrets.request(name, for_tool)` | Request secret injection authorization |
| `approval.queue.request(cap, evidence)` | Request capability approval |
| `approval.queue.decide(id, approve, reason)` | Grant or deny approval |
| `task.board` | Shared task queue (post, claim, complete) |
| `causal.chain.log(event)` | Immutable audit entry |

That's it. Sixteen primitives. No role registry. No wake predicates. No implicit routing. No disclosure classification. No evolution orchestration.

---

## How Autonoetic Properties Emerge

The autonoetic properties — self-evolving, memory-bearing, multi-agent — emerge from **agents composing these primitives**, not from the gateway hardcoding patterns:

**Delegation**: Planner agent calls `agent.spawn` based on its own instructions. Gateway validates and executes.

**Reevaluation**: Agent declares `scheduler.interval("every 20m")`. Gateway fires `tick`. Agent decides what to do.

**Evolution**: Coder agent calls `skill.store.publish`. Evaluator calls `approval.queue.decide`. Gateway enforces the gate.

**Memory**: Agent calls `memory.share`. Gateway checks ACLs and updates visibility.

The gateway doesn't understand delegation, reevaluation, or evolution. It just validates proposals and executes them. A completely different agent architecture — swarms, consensus-based delegation, ML-driven scheduling — could use the same gateway without changing a line of code.

---

## Design Principles

1. **Gateway is a dumb secure pipe** — routes messages, enforces capability boundaries, logs everything. No domain logic.

2. **Agents are pure reasoners** — they read, think, and propose. They never execute directly.

3. **Autonomy through composition** — complex behaviors emerge from agents composing simple gateway primitives.

4. **No hardcoded heuristics** — if the gateway has a concept like "lead agent" or "specialist" or "stale goal", it's coupling infrastructure to a specific orchestration pattern.

5. **Spec-driven, not code-driven** — agent behavior is defined in SKILL.md (Markdown + YAML), not in gateway code.

6. **Pluggable everything** — LLM providers, sandbox drivers, channels, capabilities. All swappable without touching core logic.

7. **Immutable audit trail** — every action logged to causal chain with hash-chain linkage. Nothing happens invisibly.

---

## What This Looks Like in Practice

```
User: "Research our competitors and build a report"

  ┌─ Planner Agent (reasoning) ─────────────────────┐
  │                                                   │
  │  1. Parse intent → multi-step plan                │
  │  2. gateway.agent.spawn("researcher", "research") │
  │  3. gateway.agent.spawn("coder", "build report")  │
  │  4. gateway.task.board.post("synthesize findings")│
  │                                                   │
  └────────────────────┬──────────────────────────────┘
                       │ proposals
                       ▼
  ┌─ Gateway (execution) ────────────────────────────┐
  │                                                   │
  │  1. Validate: agent.spawn capability? ✓           │
  │  2. Validate: "researcher" exists? ✓              │
  │  3. Spawn researcher agent                        │
  │  4. Spawn coder agent                             │
  │  5. Post to task board                            │
  │  6. Log all actions to causal chain               │
  │                                                   │
  └────────────────────┬──────────────────────────────┘
                       │ spawns
                       ▼
  ┌─ Researcher Agent ──┐  ┌─ Coder Agent ──────────┐
  │ Researches via web   │  │ Waits for research,    │
  │ Calls memory.remember│  │ then builds report     │
  │ Reports back         │  │ via sandbox.execute    │
  └──────────────────────┘  └────────────────────────┘
```

The gateway never knew what a "report" is. It never decided to spawn a researcher. It never chose a coder. It just validated and executed agent proposals, logged everything, and enforced boundaries.

---

## Summary

**Keep**: LLM drivers, sandbox drivers, channel adapters, capability-based security, artifact store, causal chain, MCP integration. These are genuinely generic infrastructure.

**Externalize**: Role registry, wake predicates, implicit routing, disclosure classification, state conventions, evolution orchestration. These belong in agent-authored SKILL.md files, not in gateway code.

**The gateway provides primitives. Agents compose them into behaviors. Autonomy emerges from composition, not from platform rules.**
