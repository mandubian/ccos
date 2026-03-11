# Gateway Architecture Principles

## Core Design Tenet: Neutral Executor, Not Rule Engine

The autonoetic gateway is designed as a **generic, neutral runtime executor**—not a business logic rule engine. This distinction is critical for maintaining agent autonomy and platform extensibility.

### ✅ What the Gateway SHOULD Do

**Generic robustness improvements** that benefit all agents:
- **Tool-name canonicalization**: Map shorthand names (`spawn`, `install`, `message`) to canonical forms (`agent.spawn`, etc.) — fixes LLM model quirks, not agent-specific rules
- **Unknown-tool error recovery**: Return structured errors instead of fatal aborts — allows sessions to continue and recover
- **Success tracking for loop-guard**: Track whether tool calls actually succeeded — prevents infinite retry loops universally
- **Error typing and resilience**: Distinguish between recoverable (resource, transient) and fatal (validation, auth) errors

These raise the floor of runtime robustness without prescribing what agents should do.

### ❌ What the Gateway Should NOT Do

**Domain-specific business logic gates** that restrict agent decision-making:
- "Reject specialized_builder delegation without concrete endpoint + auth + sample retrieval"
- "Prevent agent creation unless all preconditions are met"
- "Block research→builder transitions unless research returned non-empty data"

These hardcode assumptions about agent workflows into the platform, breaking extensibility. Different agents (or future versions) may have entirely different delegation strategies.

### Where Business Logic Belongs

**In agent SKILL.md instructions** (not platform code):
- Guardrails 8 & 9 in planner.default tell the agent: "If research has no actionable data, stop and return failure instead of delegating"
- The agent *chooses* to follow these rules through LLM instruction-following
- Different planner implementations can have different rules without changing the gateway

Example: A speculative planning agent might intentionally delegate without waiting for research success—the gateway should not prevent this. The guardrails are agent-specific, not platform-wide.

## Rationale

1. **Agent autonomy**: Agents should make routing/delegation decisions, not the platform
2. **Extensibility**: New agent types don't require platform code changes
3. **Separation of concerns**: Gateway handles execution robustness; agents handle strategy
4. **Framework-like design**: Similar to web frameworks that provide HTTP mechanics but don't enforce business logic constraints

## Historical Context

Session-6 failure flow was masked by downstream builder errors because the planner was allowed to continue delegating after empty research. The fix was **not** to add a platform gate in the gateway's lifecycle.rs, but to:
1. Make the runtime more resilient (canonical tool names, error recovery)
2. Add explicit behavioral guardrails in the planner's SKILL.md instructions

This kept the gateway generic while fixing the agent's behavior.
