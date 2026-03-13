# Pluggable Schema Enforcement Hook

## Problem

When an agent calls another agent via `agent.spawn`, the calling LLM must produce a payload matching the target agent's `io` schema. LLMs are good at reasoning but inconsistent at exact structural output. The current approach has two gaps:

1. **Lightweight validation** — gateway checks required fields and basic types only
2. **No coercion** — mismatches either pass through (garbage input to target) or fail (agent must repair)

The agent-adapter pattern (`agent-adapter.default`) handles schema mismatches by generating full wrapper agents, but this is heavyweight for common structural fixes (renamed fields, missing defaults, type coercion).

## Design Principles

1. **Gateway intercepts, gateway decides** — the schema enforcement hook sits between the calling agent's tool call and the target agent's execution
2. **Pluggable enforcers** — swap between pure code and LLM-based enforcement without changing gateway architecture
3. **Fail informatively** — when coercion is impossible, return actionable errors the LLM can reason about and repair
4. **Audit everything** — every coercion, rejection, and passthrough is logged to the causal chain

## Architecture

```
Agent → tool_call(agent.spawn) → Gateway intercepts
                                        │
                                   ┌────▼──────────────┐
                                   │  Schema Enforcer   │
                                   │  (pluggable hook)  │
                                   └──┬──────┬──────┬───┘
                                      │      │      │
                                   pass   coerce  reject
                                      │      │      │
                                      ▼      ▼      ▼
                                   execute  execute  error → agent repairs
                                   (as-is)  (fixed)  (with hints)
```

The hook is invoked after the gateway validates capabilities/permissions but before the target agent receives the payload.

## Trait Design

```rust
pub enum EnforcementResult {
    /// Payload conforms to schema — proceed unchanged
    Pass { payload: serde_json::Value },
    
    /// Payload was coerced to match schema — log the diff
    Coerced { 
        original: serde_json::Value,
        coerced: serde_json::Value,
        transformations: Vec<String>,
    },
    
    /// Cannot fix — return actionable error to calling agent
    Reject {
        errors: Vec<SchemaViolation>,
        suggested_fix: Option<String>,
    },
}

pub struct EnforcementContext {
    pub target_agent_id: String,
    pub tool_name: String,        // "agent.spawn"
    pub calling_agent_id: String,
    pub target_schema: Option<IoSchema>,
}

pub trait SchemaEnforcer: Send + Sync {
    fn enforce(
        &self,
        payload: &serde_json::Value,
        context: &EnforcementContext,
    ) -> EnforcementResult;
}
```

## Implementations

### 1. DeterministicCoercionEnforcer (pure code — ship first)

Rule-based transformations, no LLM involved:

| Transform | Example | Risk |
|-----------|---------|------|
| Add missing optional fields with defaults | `{query: "x"}` → `{query: "x", domain: null}` | Low |
| Rename misnamed fields by similarity | `{task: "x"}` → `{query: "x"}` when schema expects `query` | Medium |
| Safe type coercion | `{count: 42}` → `{count: "42"}` (number→string) | Medium |
| Unwrap over-nested objects | `{data: {query: "x"}}` → `{query: "x"}` | High |

Cannot fix: deeply nested structural mismatches, wrong array element shapes, semantic errors.

### 2. LlmCoercionEnforcer (cheap LLM — later)

Calls a fast, cheap model (GPT-4o-mini, Haiku) with:
- The malformed payload
- The target schema (JSON Schema)
- Short system prompt: "Transform input to match schema. Return only valid JSON."

Higher success rate, costs tokens. Good fallback when deterministic enforcer rejects.

## Configuration

```yaml
# gateway config
schema_enforcement:
  # Primary enforcer (always runs first)
  primary: "deterministic"
  
  # Fallback when primary rejects (null = no fallback, reject immediately)
  fallback: null  # or "llm"
  
  # Log all enforcement decisions to causal chain
  audit: true
  
  # Per-agent overrides
  agent_overrides:
    planner.default:
      fallback: "llm"  # planner calls many agents, worth the cost
```

## Error Shape Returned to Agent

When both enforcers fail, the calling agent receives structured feedback it can reason about:

```json
{
  "ok": false,
  "error_type": "schema_mismatch",
  "target": "researcher.default",
  "message": "Payload missing required field 'query'",
  "expected_schema": {
    "query": "string (required)",
    "domain": "string (optional)"
  },
  "received": {"topic": "AI competitors"},
  "hint": "Your payload used 'topic' but the target expects 'query'. Rename 'topic' to 'query'.",
  "repairable": true
}
```

The `hint` field gives the LLM a concrete repair instruction, not just a validation error.

## Causal Chain Logging

Every enforcement decision is logged:

```json
{
  "event": "schema_enforcement",
  "agent_id": "planner.default",
  "target": "researcher.default",
  "result": "coerced",
  "transformations": ["renamed 'topic' → 'query'"],
  "enforcer": "deterministic",
  "timestamp": "2026-03-13T10:30:00Z"
}
```

This enables the Auditor Agent to detect patterns: "planner.default consistently mismatches researcher.default on field naming."

## Relationship to Existing Patterns

| Pattern | Scope | When to use |
|---------|-------|-------------|
| Schema enforcement hook | Structural coercion | Every `agent.spawn` call — first line of defense |
| Agent-adapter | Behavioral + structural | Complex gaps requiring new middleware scripts |
| Structured error repair | Agent self-correction | When hook rejects — agent repairs in-session |
| `skill.describe` | Context injection | Agent proactively reads schema before calling |

The hook is the default fast path. The adapter is for complex transformations. Error repair is the fallback. `skill.describe` is optional agent initiative.

## Implementation Steps

1. Define `SchemaEnforcer` trait and `EnforcementResult` enum in `autonoetic-types`
2. Implement `DeterministicCoercionEnforcer` with required-field defaults and field renaming
3. Add `schema_enforcement` config section to `GatewayConfig`
4. Insert hook into `agent.spawn` tool execution path (after capability check, before target dispatch)
5. Add causal chain logging for all enforcement decisions
6. Add unit tests for coercion rules (pass, coerce, reject paths)
7. Add integration tests proving malformed payloads are fixed or rejected with hints
8. Update planner `SKILL.md` guidance to note that structural errors are auto-corrected when possible
9. (Later) Implement `LlmCoercionEnforcer` with fallback chain from deterministic
