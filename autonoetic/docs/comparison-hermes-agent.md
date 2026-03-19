# Autonoetic vs Hermes-Agent Comparison

> Comparing the Autonoetic agent runtime (Rust, in CCOS) with Hermes-Agent (Python, by Nous Research).

---

## 1. Core Philosophy

| Dimension | Autonoetic | Hermes-Agent |
|-----------|-----------|--------------|
| **Paradigm** | Gateway-mediated separation of powers: agents are pure reasoners, gateway is sole executor | Direct agent loop: LLM calls tools directly, no intermediary gateway |
| **Language** | Rust-first, with Python/TypeScript SDKs | Python-first |
| **Governance model** | Formal capability-based access control with policy engine | Tool approval + command allowlists (lighter-weight) |
| **Primary focus** | Auditable, portable, reproducible agent execution | Practical, daily-use AI assistant with learning loops |

## 2. Agent Architecture

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Execution model** | Two modes: **Reasoning** (LLM loop) and **Script** (deterministic, no LLM) | Single synchronous loop: `LLM → tool_calls → execute → repeat` |
| **Agent definition** | `SKILL.md` YAML frontmatter + markdown instructions | Inline prompts, `SOUL.md` persona, `AGENTS.md` instructions |
| **Manifest format** | Formal JSON Schema-capable YAML with typed capabilities, IO contracts, disclosure policies | Config-driven (config.yaml + .env) |
| **Sandboxing** | Bubblewrap, Docker, MicroVM enforced by gateway | Terminal backends: local, Docker, SSH, Modal, Daytona, Singularity |
| **Separation of powers** | **Strict**: agents cannot touch filesystem/network/secrets directly; all via gateway | **Relaxed**: tools run directly in agent context |

## 3. Memory & Learning

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Tier 1 (working)** | Content-addressable store (SHA-256), visibility model (private/session/global), artifacts for trust boundary | File-based working memory, conversation context |
| **Tier 2 (durable)** | Gateway-managed `knowledge.*` tools with provenance, scope, visibility | Procedural memory via **Skills** (autonomous creation from experience), `MEMORY.md`/`USER.md` |
| **Cross-session recall** | Session forking via `autonoetic trace fork`, causal chain replay | FTS5 session search with LLM summarization, Honcho dialectic user modeling |
| **Self-improvement** | Evolution agents (`specialized_builder`, `memory-curator`, `evolution-steward`) promote learnings | **Closed learning loop**: skills self-improve during use, autonomous skill creation, periodic nudges to persist knowledge |
| **Audit trail** | Hash-chained JSONL causal chain (immutable, verifiable) | Trajectory saving (for RL training), session persistence |

## 4. Tooling & Extensibility

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Tool count** | Core set: `content.*`, `knowledge.*`, `agent.*`, `secrets.*`, MCP tools | **40+ built-in tools**: file ops, browser, code execution, web search, voice, TTS, vision, delegation, etc. |
| **MCP support** | MCP client + server (registry, discovery, agent exposure) | MCP client integration (`tools/mcp_tool.py`) |
| **Tool registration** | Capability-declared in SKILL.md manifest, validated by policy engine | Central `tools/registry.py` with schema collection, dispatch, availability checking |
| **Extensibility pattern** | Install agents via `agent.install`, discovery via `agent.discover` | Create `tools/your_tool.py`, import in `model_tools.py`, add to `toolsets.py` |
| **Federation** | OpenFang Protocol (OFP) with HMAC handshake | Platform gateway: Telegram, Discord, Slack, WhatsApp, Signal |

## 5. Multi-Agent System

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Agent roles** | Formal role catalog: Lead (planner), Specialists (researcher, architect, coder, debugger, evaluator, auditor), Evolution (builder, curator, steward) | Delegation via `delegate_tool.py` (spawn isolated subagents) |
| **Routing** | Explicit target → session affinity → default lead (`planner.default`) | Task-based delegation, no formal routing hierarchy |
| **Agent spawning** | `agent.spawn` with structured metadata (role, expected outputs, parent goal) | `execute_code` + `delegate` for subagent creation |
| **Agent persistence** | Durable agent directories with `SKILL.md` + `runtime.lock` | Ephemeral subagents (session-scoped) |

## 6. Portability & Reproducibility

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Runtime closure** | `runtime.lock` pins exact dependency versions | `requirements.txt` / `pyproject.toml` / `uv.lock` |
| **Export format** | **Cognitive Capsule**: portable bundle of agent + runtime closure | No formal export; migration via `hermes claw migrate` from OpenClaw |
| **Remote agents** | HTTP Content API with Bearer auth, SDK auto-detects local vs remote | Gateway mirrors for messaging platforms |

## 7. Security Model

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Access control** | Capability-based: `MemoryRead`, `MemoryWrite`, `ToolInvoke`, `ShellExec`, `AgentSpawn`, `NetConnect`, etc. with pattern scoping | Command approval detection, DM pairing, container isolation |
| **Secret handling** | Vault injection (never exposed to agent, zeroized after use) | `.env` file, `hermes config set`, secrets in config |
| **Disclosure policy** | Four tiers: `public` (verbatim), `internal` (summary), `confidential` (redacted), `secret` (never) | No formal disclosure policy |
| **Policy engine** | Validates every proposal against capabilities + ACLs | Simpler allowlist-based approval |

## 8. Deployment

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Primary target** | Gateway daemon (JSON-RPC + HTTP), CLI | Interactive CLI + messaging gateway |
| **Cloud/serverless** | HTTP API for remote agents | Modal, Daytona serverless backends |
| **Messaging platforms** | OFP federation (protocol-agnostic) | Telegram, Discord, Slack, WhatsApp, Signal, Home Assistant |
| **Scheduling** | Background reevaluation with wake predicates | Built-in cron scheduler with natural language |
| **LLM providers** | 30+ providers via driver abstraction (OpenAI, Anthropic, Gemini, OpenRouter) | OpenRouter (200+ models), OpenAI, Anthropic, GLM, Kimi, MiniMax |

## 9. Research & Training

| Feature | Autonoetic | Hermes-Agent |
|---------|-----------|--------------|
| **Trajectory capture** | Causal chain JSONL with full evidence mode | Batch trajectory generation, trajectory compression |
| **RL integration** | Not a focus | Atropos RL environments, Tinker-Atropos integration |
| **Training readiness** | N/A | Designed for training next-gen tool-calling models |

## 10. Tool & Skill Repository

| Aspect | Autonoetic (current) | Autonoetic (proposed) | Hermes-Agent |
|--------|---------------------|----------------------|--------------|
| **Tool registration** | Scattered match statements in `tools.rs` | Declarative `ToolRegistry` (~150 lines Rust) | `tools/registry.py` (229 lines Python) |
| **Registration pattern** | Edit match statement + add handler | `register_tool!` macro in each tool file | `registry.register()` at import time |
| **Schema location** | Hardcoded in match arm | Co-located with handler | Co-located with handler |
| **Skill system** | None (agents are the only unit) | Shared `~/.autonoetic/skills/` with progressive disclosure | `~/.hermes/skills/` with 3-tier disclosure |
| **Skill tiers** | N/A | Tier 0: categories → Tier 1: metadata → Tier 2: full content | Same 3 tiers |
| **Skill sources** | N/A | Local, GitHub, well-known, MCP | Local, GitHub, skills.sh, ClawHub, well-known |
| **Toolset composition** | Declared in SKILL.md capabilities | Convention in SKILL.md (not gateway code) | `toolsets.py` with `includes` composition |
| **MCP integration** | Separate MCP tools | Register as first-class tool registry entries | Register as first-class registry entries |
| **New code estimate** | N/A | ~850 lines (Rust) | ~2200 lines (Python) |

### Key Insight: What Autonoetic Should Take From Hermes

1. **Registry pattern** (not implementation): Tools self-declare metadata. Gateway doesn't need to know what tools exist — it just dispatches.

2. **Progressive disclosure** (proven by Anthropic): Agent sees ~20 tokens/skill in `skill.list`, loads full content only when needed via `skill.view`. Prevents context window bloat.

3. **Skills ≠ Agents**: Hermes correctly separates skills (injectable context) from the agent loop. Autonoetic's `concepts.md` envisioned this ("Global Skill Engine Repository") but never implemented it.

4. **Multi-source discovery**: Hermes' hub pattern (local + GitHub + marketplace + well-known) is the right model. But Autonoetic should implement this as gateway primitives, not a monolithic hub module.

### What Autonoetic Does Better

1. **Toolsets as convention, not code**: Hermes' `toolsets.py` (542 lines) is overkill. Autonoetic makes toolsets a YAML convention in SKILL.md — zero gateway code.

2. **Gateway mediation**: Hermes tools run directly in agent context. Autonoetic's gateway mediation means tool dispatch has a natural enforcement point (capability checking).

3. **Rust performance**: The registry pattern in Rust is faster and has compile-time guarantees that Python can't provide.

---

## 11. Maturity & Status

| Aspect | Autonoetic | Hermes-Agent |
|--------|-----------|--------------|
| **Phase** | MVP/runtime stabilization (Phases 1-6 done, Phase 7 polishing) | Production-ready (v0.2.0 released, 3000+ tests) |
| **Language** | Rust (high performance, memory safety) | Python (rapid development, large ecosystem) |
| **Ecosystem** | CCOS-compatible, OFP federation | agentskills.io standard, OpenClaw migration path |

---

## Summary

**Autonoetic** is a **formal, governance-first runtime** built in Rust that enforces strict separation between reasoning and execution. It is designed for scenarios where auditability, reproducibility, and portable agent bundles (Cognitive Capsules) are paramount — a "operating system for agents" with capability-based security.

**Hermes-Agent** is a **practical, feature-rich AI assistant** built in Python with a built-in learning loop. It prioritizes daily utility — messaging integrations, 40+ tools, autonomous skill creation, cron scheduling, and RL training readiness — a "personal AI agent that gets better over time."

They solve overlapping but distinct problems: Autonoetic builds the **governed infrastructure** agents run on; Hermes-Agent builds the **agent itself** that users interact with.
