# Skill Architecture Decision for Autonoetic

## Question

Should Autonoetic support standalone "skills" (for example a weather API call script) as a runtime primitive distinct from agents?

## Decision

**Keep one runtime primitive: agent.**

For simple capabilities, use **script-mode agents** (`execution_mode: "script"`).  
Do not introduce a separate standalone skill runtime entity at this stage.

## Rationale

### 1) Security and governance already anchor on AgentManifest

The current gateway contracts rely on an agent manifest for:
- capability validation and policy checks
- install/approval lifecycle
- runtime identity and audit linkage
- sandbox execution context and directory ownership

Creating a second primitive would duplicate these controls and increase drift risk.

### 2) Script-mode agents are already the lightweight skill model

Script agents already provide:
- no LLM requirement
- direct deterministic execution path
- same causal-chain and approval semantics as the rest of the platform

So "basic skill" behavior is already available without architectural branching.

### 3) Operational simplicity matters

"Everything is an agent" keeps:
- one install path
- one policy model
- one approval model
- one discovery/routing model

Two runtime primitives would reduce conceptual clarity and increase maintenance burden.

## Important Caveat (Security Reality)

Before broadening adoption of script skills, prioritize hardening script execution isolation.

Current code paths indicate bubblewrap execution for scripts is still in a transitional state.  
This means the key risk is sandbox hardening, not lack of a "skill" primitive.

## Recommended Product Direction

Keep runtime primitive = **agent**, but add a **skill-profile UX wrapper**:

- optional CLI/tool sugar (for example `skill.install`) that compiles into `agent.install`
- emits a script-mode agent manifest and files under the hood
- applies opinionated defaults for "simple skills"

This gives a lighter developer experience without splitting runtime architecture.

## Skill-Profile Guardrails (Recommended)

When using the skill-profile wrapper:

1. require explicit `script_entry`
2. require explicit capability declarations
3. if network is requested:
   - require explicit host allowlist (no wildcard by default)
   - require approval by policy
4. keep all execution sandboxed and auditable through existing agent paths

## Decision Guide

| Use Case | Entity | Reasoning |
|----------|--------|-----------|
| "Get weather for Paris" | Script agent (skill-profile optional) | Deterministic + reusable |
| "Check BTC price every hour" | Script agent + `BackgroundReevaluation` | Scheduled, no LLM |
| "Research competitors" | Reasoning agent | Needs judgment and synthesis |
| Core platform operations | Gateway primitives | Infrastructure-level concerns |

## Final Position

For synthetic skills created via install flows:

1. **Do not add a separate runtime skill entity**
2. **Use script-mode agents as the skill abstraction**
3. **Add UX-level skill-profile tooling if needed**
4. **Prioritize sandbox hardening before scaling script-skill usage**
