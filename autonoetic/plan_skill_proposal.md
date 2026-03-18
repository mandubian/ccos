# Skill Architecture Decision for Autonoetic

## Question

Should autonoetic allow creating "skills" (simple scripts like "get the weather") as standalone entities, instead of always creating full agents?

## Decision: Script-Mode Agents ARE the Skills

**No separate skill entity needed.** The existing `execution_mode: "script"` agent is already the right abstraction.

### Why

The gateway security infrastructure (`policy.rs:308-316`) requires:
1. A capabilities array (for policy validation)
2. An agent directory (for sandbox mount)
3. An entry script path

All of these live in `AgentManifest`. There is no separate "skill manifest" type in the gateway.

### Overhead Comparison

For a script-mode agent, the overhead vs a theoretical standalone skill:
- `SKILL.md` with frontmatter (required — this IS the skill definition)
- A directory (required — where else do you put the script?)
- A `runtime.lock` (required by install flow)

No LLM config, no reasoning loop, no session management. The script runs directly.

### What Works Without Changes

| Property | Status |
|----------|--------|
| Security (capabilities) | Already works |
| Sandbox execution | Already works |
| Causal chain audit | Already works |
| SDK access (memory, content) | Available if needed |
| Discovery via `agent.discover` | Works |
| Evolution to reasoning mode | Change `execution_mode` field |

### Key Code Paths

- `autonoetic-types/src/agent.rs:96-104` — `ExecutionMode::Script` variant
- `autonoetic-gateway/src/policy.rs:308-316` — `PolicyEngine` takes `AgentManifest`
- `autonoetic-gateway/src/sandbox.rs:81` — `spawn(agent_dir, entrypoint)`
- `autonoetic-gateway/src/runtime/tools.rs:6446` — Test: `test_agent_install_script_mode_allows_no_llm_config`

### Decision Guide

| Use Case | Entity | Reasoning |
|----------|--------|-----------|
| "Get weather for Paris" | Script agent | Fast, deterministic, reusable |
| "Check BTC price every hour" | Script agent + `BackgroundReevaluation` | Scheduled, no LLM |
| "Research competitors" | Reasoning agent | Needs judgment |
| Native gateway tools | Gateway primitives | Core infrastructure |

## Decision

For autonoetic's synthetic skills (created by agents via `agent.install`):
1. **Keep using script-mode agents** — they ARE the skill concept
2. **Do not create a separate skill entity** — it would duplicate security plumbing
3. **Enhance the script-agent fast path** — make it lighter where possible
