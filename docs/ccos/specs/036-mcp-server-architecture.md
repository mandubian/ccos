# CCOS as MCP Server: Agent-Driven Planning Architecture

> **Status**: Implemented
> **Date**: 2026-01-12
> **Author**: AI-Assisted Implementation

## Vision

CCOS has been transformed into a **backend cognitive engine** exposed via MCP. External agents (Chat LLMs, IDE agents) act as the "Dialogue Planner," orchestrating CCOS primitives conversationally.

```
┌─────────────────────────────────────────────────────────────────┐
│                        Chat Interface                           │
│  (Claude, Gemini, IDE Agent, etc.)                              │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  "External Brain" / Dialogue Controller                 │    │
│  │  - Holds conversation state                             │    │
│  │  - Decides when to analyze, resolve, execute            │    │
│  │  - Presents results to user                             │    │
│  └────────────────────────┬────────────────────────────────┘    │
└───────────────────────────┼─────────────────────────────────────┘
                            │ MCP Protocol
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CCOS MCP Server                            │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │  Discovery   │  │  Session     │  │  Execution           │   │
│  │  Tools       │  │  Tools       │  │  Tools               │   │
│  │              │  │              │  │                      │   │
│  │ ccos_search  │  │ session_start│  │ execute_capability   │   │
│  │ suggest_apis │  │ session_plan │  │ execute_plan         │   │
│  │ decompose    │  │ session_end  │  │ synthesize_cap       │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Core Engine (Rust)                                     │    │
│  │  - ModularPlanner, IntentGraph, CapabilityMarketplace   │    │
│  │  - PlanArchive, GovernanceKernel, CausalChain           │    │
│  │  - AgentMemory (Tangible Learning)                      │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implemented MCP Tools

### Phase 1: Discovery & Planning

#### `ccos_search`
Search for available capabilities by query, ID pattern, or domains.

```json
{
  "name": "ccos_search",
  "inputSchema": {
    "query": "string (required)",
    "domains": "string[] (optional)",
    "limit": "integer (optional, default 10)",
    "min_score": "number (optional, default 0.0)"
  }
}
```

#### `ccos_plan`
Decompose a goal into sub-intents using LLM. Identifies capability gaps.

```json
{
  "name": "ccos_plan",
  "inputSchema": {
    "goal": "string (required)"
  }
}
```

#### `ccos_decompose`
Find capabilities that can fulfill a specific intent description.

```json
{
  "name": "ccos_decompose",
  "inputSchema": {
    "intent": "string (required)",
    "domain": "string (optional)",
    "min_score": "number (optional, default 0.5)"
  }
}
```

#### `ccos_suggest_apis`
Ask LLM to suggest well-known APIs for a given goal (pure suggestion, no auto-approval).

```json
{
  "name": "ccos_suggest_apis",
  "inputSchema": {
    "query": "string (required)"
  }
}
```

---

### Phase 2: Execution & Session Management

#### `ccos_execute_capability`
**PRIMARY TOOL**. Execute a capability with JSON inputs. Automatically manages session state.

```json
{
  "name": "ccos_execute_capability",
  "inputSchema": {
    "capability_id": "string (required)",
    "inputs": "object (required)",
    "session_id": "string (optional)",
    "original_goal": "string (optional)"
  }
}
```

#### `ccos_session_start`
Start a new planning/execution session explicitly.

```json
{
  "name": "ccos_session_start",
  "inputSchema": {
    "goal": "string (required)",
    "context": "object (optional)"
  }
}
```

#### `ccos_session_plan`
Get the accumulated RTFS plan from a session.

```json
{
  "name": "ccos_session_plan",
  "inputSchema": {
    "session_id": "string (required)"
  }
}
```

#### `ccos_session_end`
End a session and optionally save the plan.

```json
{
  "name": "ccos_session_end",
  "inputSchema": {
    "session_id": "string (required)",
    "save_as": "string (optional)"
  }
}
```

---

### Phase 3: Synthesis & Learning

#### `ccos_consolidate_session`
Convert a session trace into a reusable **Agent Capability**.

```json
{
  "name": "ccos_consolidate_session",
  "inputSchema": {
    "session_id": "string (required)",
    "agent_name": "string (required)",
    "description": "string (optional)"
  }
}
```

#### `ccos_synthesize_capability`
Synthesize a new RTFS capability using LLM.

```json
{
  "name": "ccos_synthesize_capability",
  "inputSchema": {
    "description": "string (required)",
    "capability_name": "string (optional)",
    "input_schema": "object (optional)",
    "output_schema": "object (optional)"
  }
}
```

#### `ccos_log_thought`
Record reasoning into Agent Memory.

```json
{
  "name": "ccos_log_thought",
  "inputSchema": {
    "thought": "string (required)",
    "plan_id": "string (optional)",
    "is_failure": "boolean (optional)"
  }
}
```

#### `ccos_record_learning`
Explicitly record a learned pattern.

```json
{
  "name": "ccos_record_learning",
  "inputSchema": {
    "pattern": "string (required)",
    "context": "string (required)",
    "outcome": "string (required)",
    "confidence": "number (optional)"
  }
}
```

#### `ccos_recall_memories`
Recall memories by tag.

```json
{
  "name": "ccos_recall_memories",
  "inputSchema": {
    "tags": "string[] (required)",
    "limit": "integer (optional)"
  }
}
```

---

### Phase 4: Governance Discovery

#### `ccos_get_constitution`
Get the system constitution rules.

```json
{
  "name": "ccos_get_constitution",
  "inputSchema": {}
}
```

#### `ccos_get_guidelines`
Get the official agent guidelines.

```json
{
  "name": "ccos_get_guidelines",
  "inputSchema": {}
}
```

---

## Resources (Read-Only State)

| Resource URI | Description |
|--------------|-------------|
| `ccos://capabilities/list` | All registered capabilities |
| `ccos://memory/context` | Current working memory state |

---

## Interaction Flow (Implemented)

**User**: "List my GitHub issues and group them by label"

**Agent (Chat LLM)**:
1. Calls `ccos_plan` -> Gets sub-intents.
2. Identifies `list_issues` capability via `ccos_decompose`.
3. Calls `ccos_execute_capability("mcp.github/list_issues", ...)` -> Result.
4. Cannot find "group by label" capability.
5. Calls `ccos_synthesize_capability` for grouping logic.
6. Calls `ccos_execute_capability("synthesized.group_by_label", ...)` -> Result.
7. Calls `ccos_session_end` to save the workflow.
8. (Optional) Calls `ccos_consolidate_session` to turn this flow into a permanent Agent.
