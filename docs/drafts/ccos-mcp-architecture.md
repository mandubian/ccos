# CCOS as MCP Server: Agent-Driven Planning Architecture

> **Status**: Draft  
> **Date**: 2026-01-02  
> **Author**: AI-Assisted Design Session

## Vision

Transform CCOS from a monolithic planner into a **backend cognitive engine** exposed via MCP. External agents (Chat LLMs, IDE agents, other systems) become the "Dialogue Planner," orchestrating CCOS primitives conversationally.

```
┌─────────────────────────────────────────────────────────────────┐
│                        Chat Interface                           │
│  (Claude, Gemini, IDE Agent, etc.)                              │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  "Meta-Agent" / Dialogue Controller                     │    │
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
│  │  Analysis    │  │  Resolution  │  │  Execution           │   │
│  │  Tools       │  │  Tools       │  │  Tools               │   │
│  │              │  │              │  │                      │   │
│  │ analyze_goal │  │ resolve      │  │ generate_plan        │   │
│  │ decompose    │  │ discover     │  │ validate_plan        │   │
│  │              │  │ synthesize   │  │ execute_plan         │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Core Engine (Rust)                                     │    │
│  │  - ModularPlanner, IntentGraph, CapabilityMarketplace  │    │
│  │  - PlanArchive, GovernanceKernel, CausalChain          │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Why Not Just Generate Python?

| Dimension | Python Generation | CCOS + RTFS |
|-----------|-------------------|-------------|
| **Short-term utility** | ✅ Excellent | ❌ Overhead |
| **Long-term memory** | ❌ Ephemeral scripts | ✅ IntentGraph, PlanArchive |
| **Governance at scale** | ❌ Review per-run | ✅ Trust per-capability |
| **Multi-agent coordination** | ❌ Hard | ✅ Shared semantic state |
| **Self-improvement** | ❌ No formal representation | ✅ Plans are data |
| **Effect analysis** | ❌ Can do anything | ✅ Explicit `[:io :network]` |

**Key Insight**: Python wins for ad-hoc scripting. CCOS wins when you need **persistent knowledge**, **governance**, and **composable, trusted capabilities**.

---

## Proposed MCP Tools

### Phase 1: Analysis & Exploration

#### `ccos/analyze_goal`
Decompose a natural language goal into structured sub-intents.

```json
{
  "name": "ccos/analyze_goal",
  "inputSchema": {
    "goal": "string",
    "max_depth": "integer (optional, default 1)"
  }
}
```

**Returns**:
```json
{
  "feasibility": 0.85,
  "intents": [
    { "id": "1", "description": "list github issues", "likely_tool": "mcp.github/list_issues" },
    { "id": "2", "description": "summarize with llm", "likely_tool": "missing" }
  ],
  "missing_domains": ["summarization"]
}
```

#### `ccos/discover_capabilities`
Search for available tools matching a query or domain.

```json
{
  "name": "ccos/discover_capabilities",
  "inputSchema": {
    "query": "string",
    "domains": "string[] (optional)"
  }
}
```

---

### Phase 2: Resolution & Synthesis

#### `ccos/resolve_intent`
Find the best capability to fulfill an intent.

```json
{
  "name": "ccos/resolve_intent",
  "inputSchema": {
    "intent_description": "string",
    "constraints": "object (optional)"
  }
}
```

**Returns**:
```json
{
  "status": "resolved | missing | ambiguous",
  "capability_id": "mcp.github/list_issues",
  "confidence": 0.92,
  "alternatives": []
}
```

#### `ccos/synthesize_capability`
Create a new capability from a specification or code.

```json
{
  "name": "ccos/synthesize_capability",
  "inputSchema": {
    "description": "string",
    "rtfs_code": "string (optional)",
    "input_schema": "object (optional)",
    "output_schema": "object (optional)"
  }
}
```

---

### Phase 3: Formalization & Execution

#### `ccos/generate_plan`
Generate RTFS code from resolved intents.

```json
{
  "name": "ccos/generate_plan",
  "inputSchema": {
    "goal": "string",
    "resolved_steps": [
      { "intent_id": "1", "capability_id": "mcp.github/list_issues", "params": {} }
    ]
  }
}
```

**Returns**:
```clojure
(plan "list-github-issues"
  :goal "List all open issues"
  (let [issues (call "mcp.github/list_issues" {:state "open"})]
    (call "ccos.io.println" {:text issues})))
```

#### `ccos/validate_plan`
Run static analysis and governance checks on a plan.

```json
{
  "name": "ccos/validate_plan",
  "inputSchema": {
    "rtfs_code": "string"
  }
}
```

#### `ccos/execute_plan`
Submit a plan for execution.

```json
{
  "name": "ccos/execute_plan",
  "inputSchema": {
    "plan_id": "string (optional, if archived)",
    "rtfs_code": "string (optional, if inline)"
  }
}
```

---

## Resources (Read-Only State)

| Resource URI | Description |
|--------------|-------------|
| `ccos://capabilities/list` | All registered capabilities |
| `ccos://intents/{id}` | Specific intent from IntentGraph |
| `ccos://plans/{id}` | Archived plan by ID |
| `ccos://memory/context` | Current working memory state |

---

## Implementation Roadmap

### Phase 1: Core MCP Server (Week 1-2)
- [ ] Create `ccos/src/mcp/planning_server.rs`
- [ ] Implement `analyze_goal`, `discover_capabilities`, `resolve_intent`
- [ ] Wire to existing `ModularPlanner` and `ResolutionStrategy`

### Phase 2: Synthesis & Execution (Week 3-4)
- [ ] Implement `synthesize_capability`, `generate_plan`
- [ ] Implement `validate_plan`, `execute_plan`
- [ ] Add resource endpoints for state inspection

### Phase 3: Deprecate Internal Dialogue (Week 5)
- [ ] Mark `DialoguePlanner` as legacy
- [ ] Update demos to use MCP client
- [ ] Write migration guide

---

## Example Interaction Flow

**User**: "List my GitHub issues and group them by label"

**Agent (Chat LLM)**:
1. Calls `ccos/analyze_goal` → Gets 3 intents: `list_issues`, `group_by_label`, `display`
2. Shows user: "I'll need to: 1) Fetch issues, 2) Group by label, 3) Display. OK?"
3. User confirms.
4. Calls `ccos/resolve_intent("list issues")` → `mcp.github/list_issues` ✅
5. Calls `ccos/resolve_intent("group by label")` → `missing` ❌
6. Agent writes RTFS for grouping, calls `ccos/synthesize_capability`
7. Calls `ccos/generate_plan` → Gets RTFS code
8. Shows plan to user, user approves
9. Calls `ccos/execute_plan` → Returns result

---

## Open Questions

1. **Auth passthrough**: How do MCP auth tokens flow to CCOS for external APIs?
2. **Streaming**: Should `execute_plan` stream results or return at completion?
3. **Governance integration**: How does the approval queue interact with external agents?
