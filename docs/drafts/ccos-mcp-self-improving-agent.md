# CCOS as a Self-Improving MCP Cognitive Substrate

> **Status**: Draft  
> **Date**: 2026-01-03  
> **Author**: AI-Assisted Design Session

---

## Executive Summary

This document proposes transforming CCOS from a monolithic planner into a **dynamic MCP server** where all CCOS features become callable tools for external chat agents. The architecture enables:

1. **Dynamic capability synthesis** — New capabilities are generated and served as MCP tools at runtime
2. **Self-improvement** — The system learns from execution history and evolves its own planning strategies
3. **Multi-party governance** — Humans, LLMs, or policy engines participate in approval via dialogue
4. **RTFS as native language** — Logic and data exchange in a unified, LLM-friendly format
5. **Isolated execution** — Synthesized code runs in secure sandboxes with explicit effect tracking

> **Critical Question**: Is this vision over-engineered, or does it solve problems that "chat agent + Python + JSON" cannot?

---

## Table of Contents

1. [Vision & Architecture](#vision--architecture)
2. [Why Not Just Generate Python?](#why-not-just-generate-python)
3. [Core Subsystems as MCP](#core-subsystems-as-mcp)
4. [Dynamic Capability Synthesis](#dynamic-capability-synthesis)
5. [The Learning Loop](#the-learning-loop)
6. [Governance as Dialogue Protocol](#governance-as-dialogue-protocol)
7. [The Meta-Planner](#the-meta-planner)
8. [Context Horizon & Working Memory](#context-horizon--working-memory)
9. [Subconscious: Background Intelligence](#subconscious-background-intelligence)
10. [Isolation & Security](#isolation--security)
11. [Auto-Repair & Grounded Execution](#auto-repair--grounded-execution)
12. [RTFS: Beyond JSON](#rtfs-beyond-json)
13. [Complete Tool Landscape](#complete-tool-landscape)
14. [Over-Engineering Analysis](#over-engineering-analysis)
15. [Open Questions](#open-questions)
16. [Implementation Roadmap](#implementation-roadmap)

---

## Vision & Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          External Chat Agent                                 │
│               (Claude, Gemini, IDE Agent, Custom LLM)                       │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  "Meta-Agent" / Dialogue Controller                                   │  │
│  │  - Holds conversation state with user                                 │  │
│  │  - Decides when to analyze, resolve, execute, synthesize             │  │
│  │  - Presents results, handles clarification                           │  │
│  │  - Can evolve its own strategy by querying ccos/meta_plan            │  │
│  └────────────────────────────────┬──────────────────────────────────────┘  │
└───────────────────────────────────┼─────────────────────────────────────────┘
                                    │ MCP Protocol (stdio/SSE)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CCOS MCP Server (Dynamic)                           │
│                                                                             │
│  ┌─────────────────┐  ┌────────────────┐  ┌────────────────────────────┐   │
│  │  Core Tools     │  │  Synthesis     │  │  Dynamically Added         │   │
│  │  (static)       │  │  Engine        │  │  Tools (runtime)           │   │
│  │                 │  │                │  │                            │   │
│  │ analyze_goal    │  │ synthesize_    │  │ my_custom_workflow         │   │
│  │ discover_caps   │──▶ capability ────│──▶ summarize_github_issues   │   │
│  │ resolve_intent  │  │                │  │ deploy_to_staging          │   │
│  │ generate_plan   │  │ synthesize_    │  │ ...                        │   │
│  │ execute_plan    │  │ strategy       │  │                            │   │
│  │ query_memory    │  │                │  │ (MCP tool registry grows)  │   │
│  │ meta_plan       │  └────────────────┘  └────────────────────────────┘   │
│  │ request_govnce  │                                                       │
│  └─────────────────┘                                                       │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │                    CCOS Core Engine (Rust)                            │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐  │  │
│  │  │ Modular     │ │ Intent      │ │ Capability  │ │ Governance      │  │  │
│  │  │ Planner     │ │ Graph       │ │ Marketplace │ │ Kernel          │  │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────┘  │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐  │  │
│  │  │ Plan        │ │ Causal      │ │ Working     │ │ Context         │  │  │
│  │  │ Archive     │ │ Chain       │ │ Memory      │ │ Horizon         │  │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────┘  │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────────────┐  │  │
│  │  │ Subconscious│ │ RTFS        │ │ Isolation / Sandbox Executor    │  │  │
│  │  │ (bg analysis)│ │ Runtime     │ │                                 │  │  │
│  │  └─────────────┘ └─────────────┘ └─────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Insight

The **Dialogue Planner** role is externalized to the chat agent. CCOS focuses on:
- Semantic computation (goal decomposition, resolution, synthesis)
- Persistent knowledge (IntentGraph, PlanArchive, WorkingMemory)
- Governance enforcement (Constitution, approval queue)
- Self-improvement (learning from CausalChain)

---

## Why Not Just Generate Python?

| Dimension | Python + JSON | CCOS + RTFS |
|-----------|---------------|-------------|
| **Immediate utility** | ✅ Excellent—runs anywhere | ❌ Requires CCOS runtime |
| **Long-term memory** | ❌ Scripts are ephemeral | ✅ PlanArchive, IntentGraph persist |
| **Governance at scale** | ❌ Review every script | ✅ Trust per-capability, Constitution rules |
| **Multi-agent coordination** | ❌ Hard—no shared state | ✅ Shared semantic state via MCP resources |
| **Self-improvement** | ❌ No formal representation | ✅ Plans are data, strategies evolve |
| **Effect analysis** | ❌ Python can do anything | ✅ Explicit `[:io :network]` declarations |
| **Composition** | ❌ Copy-paste, manual glue | ✅ Capabilities compose via RTFS |
| **Logic exchange** | ❌ JSON = data only | ✅ RTFS = data + executable logic |

### When Python Wins

- Ad-hoc scripting with no reuse
- One-off data processing
- Integration with Python ecosystem (ML libraries, pandas, etc.)
- User is a developer who reviews code

### When CCOS Wins

- **Persistent cognitive substrate** — the agent accumulates knowledge
- **Governed automation** — actions must be auditable and constrained
- **Multi-agent systems** — shared state, coordinated planning
- **Self-programming** — the system writes and improves itself

---

## Core Subsystems as MCP

### Phase 1: Analysis & Exploration

#### `ccos/analyze_goal`
Decompose natural language into structured sub-intents.
> **Note**: Capable LLMs (e.g., Claude 3.5 Sonnet) may skip this and jump directly to `resolve_intent` if the goal is clear. This tool is primarily for bootstrapping or handling ambiguous requests.

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
    { "id": "2", "description": "summarize results", "likely_tool": "missing" }
  ],
  "missing_domains": ["summarization"]
}
```

#### `ccos/discover_capabilities`
Search for tools matching a query or domain.

```json
{
  "name": "ccos/discover_capabilities",
  "inputSchema": {
    "query": "string",
    "domains": "string[] (optional)",
    "include_external": "boolean (optional, default true)"
  }
}
```

**Maps to**: `MCPDiscoveryService.discover_tools()` in [mcp/core.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/mcp/core.rs)

#### `ccos/search_tools`
Full-text search over capability descriptions and schemas.

```json
{
  "name": "ccos/search_tools",
  "inputSchema": {
    "query": "string",
    "limit": "integer (optional, default 10)"
  }
}
```

#### `ccos/log_thought`
Record the agent's reasoning process into the CausalChain.

> **Why call this?**:
> 1. **Long-Term Memory**: Your thoughts become part of the `ContextHorizon`. When you revisit this task later, `ccos/get_context_for_task` will return your reasoning, preventing context loss.
> 2. **Collaboration**: In multi-agent scenarios, other agents can understand *why* you made a decision.
> 3. **Better Repair**: If execution fails, CCOS uses your logged thoughts to provide better `suggest_improvements` (e.g., "You thought X, but reality was Y").

```json
{
  "name": "ccos/log_thought",
  "inputSchema": {
    "plan_id": "string (optional)",
    "thought": "string",
    "context": "object (optional, e.g. { model: 'claude-3-5', confidence: 0.8 })"
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
  "alternatives": [],
  "risk_assessment": { "level": "low", "factors": [] }
}
```

**Maps to**: `capabilities_v2.rs::resolve_intent` logic

#### `ccos/synthesize_capability`
Create a new RTFS capability from specification.

```json
{
  "name": "ccos/synthesize_capability",
  "inputSchema": {
    "description": "string",
    "input_schema": "object (optional)",
    "output_schema": "object (optional)",
    "rtfs_hint": "string (optional, partial RTFS code)"
  }
}
```

**Returns**:
```json
{
  "capability_id": "generated/summarize_github_issues",
  "rtfs_code": "(defcap ...)",
  "registered": true,
  "governance_status": "approved | pending | requires_human"
}
```

**Key**: The new capability becomes immediately callable as `ccos/generated/summarize_github_issues` MCP tool.

---

### Phase 3: Planning & Execution

#### `ccos/generate_plan`
Generate executable RTFS from resolved intents.

```json
{
  "name": "ccos/generate_plan",
  "inputSchema": {
    "goal": "string",
    "resolved_steps": [
      { "intent_id": "1", "capability_id": "...", "params": {} }
    ]
  }
}
```

**Returns RTFS**:
```clojure
(plan "list-and-summarize"
  :goal "List issues and summarize"
  (let [issues (call "mcp.github/list_issues" {:state "open"})]
    (call "generated/summarize" {:items issues})))
```

#### `ccos/validate_plan`
Run static analysis and governance checks.

```json
{
  "name": "ccos/validate_plan",
  "inputSchema": {
    "rtfs_code": "string"
  }
}
```

**Returns**:
```json
{
  "valid": true,
  "constitution_violations": [],
  "risk_score": 0.15,
  "declared_effects": ["io.read", "network.get"],
  "semantic_judgment": "Plan aligns with stated goal"
}
```

**Maps to**: `GovernanceKernel.validate_against_constitution()` and `PlanJudge` in [governance_kernel.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/governance_kernel.rs)

#### `ccos/execute_plan`
Submit a plan for execution.

```json
{
  "name": "ccos/execute_plan",
  "inputSchema": {
    "plan_id": "string (optional, if archived)",
    "rtfs_code": "string (optional, if inline)",
    "stream_progress": "boolean (optional, default false)"
  }
}
```

**Streaming** (if enabled):
```json
{"type": "step_started", "step": 1, "capability": "mcp.github/list_issues"}
{"type": "step_completed", "step": 1, "partial_result": {...}}
{"type": "plan_completed", "result": {...}}
```

---

## Dynamic Capability Synthesis

The radical idea: **CCOS's MCP tool list grows at runtime**.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Capability Lifecycle                                      │
│                                                                             │
│  1. Chat agent discovers missing capability                                 │
│     └─▶ ccos/resolve_intent returns "missing"                              │
│                                                                             │
│  2. Agent requests synthesis                                                │
│     └─▶ ccos/synthesize_capability({ description: "..." })                 │
│                                                                             │
│  3. CCOS generates RTFS, runs governance checks                            │
│     └─▶ SynthesisRiskAssessment.assess() in governance_kernel.rs           │
│     └─▶ If high-risk: returns { governance_status: "requires_human" }      │
│                                                                             │
│  4. If approved, capability is registered                                   │
│     └─▶ CapabilityMarketplace.register()                                   │
│     └─▶ MCP tool `ccos/generated/new_capability` now callable              │
│                                                                             │
│  5. Chat agent (or other agents) can now call it                           │
│     └─▶ ccos/generated/new_capability({ ... })                             │
│                                                                             │
│  6. Execution is logged to CausalChain for future learning                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Example Flow

```
User: "Summarize my GitHub issues by priority"

Agent:
1. Calls ccos/analyze_goal → intents: [list_issues, categorize_by_priority, summarize]
2. Calls ccos/resolve_intent("categorize by priority") → "missing"
3. Calls ccos/synthesize_capability({ description: "Group items by a priority field" })
4. Gets back: { capability_id: "generated/group_by_priority", governance_status: "approved" }
5. Calls ccos/generate_plan with all resolved capabilities
6. Calls ccos/execute_plan
7. Returns summarized result to user

Next time: "group_by_priority" capability already exists—reused without synthesis.
```

---

## The Learning Loop

CCOS maintains three interconnected learning systems:

### 1. CausalChain → WorkingMemory → Future Planning

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Learning Architecture                                │
│                                                                             │
│   Execution                  Distillation               Recall              │
│   ─────────                  ────────────               ──────              │
│                                                                             │
│   CausalChain                WorkingMemory              Planning            │
│   (append-only log)    →     (compact wisdom)     →    (informed by past)  │
│                                                                             │
│   • Every action             • Semantic tags           • "What worked?"    │
│   • Full provenance          • Token-aware             • "Similar goals?"  │
│   • Success/failure          • Queryable               • "Known failures?" │
│   • Time, cost, effect       • Reduced from ledger     • Strategy choice   │
│                                                                             │
│   [ingestor.rs]              [working_memory/]          [resolve_intent]   │
│   [causal_chain/]            [boundaries.rs]            [meta_plan]        │
└─────────────────────────────────────────────────────────────────────────────┘
```

**MCP Tools**:
```json
ccos/query_memory
  - input: { tags: ["github", "error"], k: 5, time_window_s: 86400 }
  - output: { entries: [...wisdom entries...] }

ccos/learn_from_execution
  - input: { plan_id, outcome: "success" | "failure", notes }
  - output: { memory_entries_created, patterns_detected }

ccos/find_similar_plans
  - input: { goal_description }
  - output: { similar_plans: [...with success rates...] }
```

**Maps to**: [working_memory/](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/working_memory/mod.rs), [ingestor.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/working_memory/ingestor.rs)

### 2. PlanArchive: Reusable Plans

Successful plans are archived with metadata:
- Original goal
- Resolution map
- Execution outcome
- Cost/time metrics

Chat agents can:
```json
ccos/archive_plan
  - input: { rtfs_code, goal, tags }
  - output: { plan_id }

ccos/replay_plan
  - input: { plan_id, overrides: {} }
  - output: { result }
```

### 3. Self-Correcting via Error Analysis

When execution fails:
1. Error logged to CausalChain
2. WorkingMemory gets "failure pattern" entry
3. Next similar goal → agent queries `ccos/find_similar_plans` → sees past failures
4. Agent can call `ccos/synthesize_from_error` to auto-generate fix

```json
ccos/synthesize_from_error
  - input: { failed_plan_id, error_pattern }
  - output: { repaired_capability_id, patch_rtfs }
```

---

## Governance as Dialogue Protocol

### The Constitution

CCOS has a `Constitution` ([governance_kernel.rs L159-253](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/governance_kernel.rs#L159-253)) that defines:

```rust
pub struct ConstitutionRule {
    pub id: String,
    pub description: String,
    pub match_pattern: String,  // Glob pattern for capability ID
    pub action: RuleAction,     // Allow | Deny | RequireHumanApproval
}
```

Default rules include:
- `ccos.cli.config.*` → RequireHumanApproval
- `ccos.cli.discovery.*` → Allow
- `*launch-nukes*` → Deny("Rule against global thermonuclear war")

### Governance as MCP Tool

```json
ccos/request_governance_decision
  - input: {
      decision_type: "approval" | "clarification" | "strategy",
      context: { plan_id, action, risk_score },
      participants: ["human:alice", "llm:ethics-model"],
      timeout_s: 300
    }
  - output: { decision_id }

ccos/provide_governance_response
  - input: { decision_id, participant_id, response }
  - output: { status, remaining_participants, final_decision }
```

### Multi-Party Governance

The dialogue protocol allows:
- **Humans**: Approve high-risk operations
- **LLM Ethics Agents**: Review synthesized capabilities
- **Policy Engines**: Auto-approve based on rules

```
Chat Agent                    CCOS                         Human
    │                          │                             │
    │ synthesize_capability    │                             │
    │ (high-risk detected)     │                             │
    │─────────────────────────▶│                             │
    │                          │ request_governance_decision │
    │                          │ ({type: "approval",         │
    │                          │   participants: ["human"]}) │
    │                          │────────────────────────────▶│
    │                          │                             │
    │                          │   provide_governance_response
    │                          │◀────────────────────────────│
    │                          │                             │
    │◀─────────────────────────│                             │
    │ { governance_status:     │                             │
    │   "approved" }           │                             │
```

---

## The Meta-Planner

### Evolving Planning Strategies

The meta-planner lets the chat agent **ask CCOS for strategic advice**:

```json
ccos/meta_plan
  - input: {
      goal: "Complex multi-step task",
      constraints: { time_budget_s, risk_tolerance },
      context: { past_failures, known_capabilities }
    }
  - output: {
      recommended_strategy: "iterative-refine",
      strategy_rtfs: "(strategy ...)",
      reasoning: "Based on 3 similar past goals, iterative works best",
      alternatives: [...]
    }
```

### Strategies as RTFS

Planning strategies are themselves RTFS code:

```clojure
(strategy "iterative-plan-refine"
  :description "Generate plan, validate, ask user, refine"
  :when [:goal-has :high-ambiguity]
  (fn [goal ctx]
    (let [plan (call "ccos/generate_plan" {:goal goal})]
      (if (< (get plan :confidence) 0.7)
        (let [feedback (call "ccos/request_governance_decision" 
                             {:type "clarification"})]
          (recur (refine-goal goal feedback)))
        plan))))

(strategy "fail-fast-synthesize"
  :description "Try execution, on failure synthesize missing capability"
  :when [:goal-refers :unknown-domain]
  (fn [goal ctx]
    (try
      (call "ccos/execute_plan" {:goal goal})
      (catch :missing-capability err
        (call "ccos/synthesize_capability" {:from-error err})))))
```

### Learning Which Strategies Work

```json
ccos/evaluate_strategy_outcome
  - input: { strategy_id, goal, outcome, metrics }
  - output: { updated_confidence }

ccos/synthesize_new_strategy
  - input: { failed_patterns: [...], desired_behavior }
  - output: { new_strategy_id, strategy_rtfs }
```

The chat agent literally **evolves its own planning approach** by generating new strategies.

---

## Context Horizon & Working Memory

### The Problem

LLMs have finite context windows. A long-running agent accumulates:
- Intent history
- Execution logs
- Capability knowledge
- Failure patterns

Without management, this exceeds context limits.

### The Solution: ContextHorizonManager

From [context_horizon.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/context_horizon.rs):

```rust
pub struct ContextHorizonManager {
    intent_graph: IntentGraphVirtualization,
    causal_chain: CausalChainDistillation,
    plan_abstraction: PlanAbstraction,
    working_memory: Arc<Mutex<WorkingMemory>>,
    config: ContextHorizonConfig,  // max_tokens, per-component budgets
}
```

**Key capabilities**:
1. **Semantic search** over IntentGraph
2. **Distillation** of CausalChain into compact wisdom
3. **Plan abstraction** (replace concrete steps with handles)
4. **Token budgeting** (reduce when over limit)

### MCP Exposure

```json
ccos/get_context_for_task
  - input: { task_description, max_tokens }
  - output: {
      relevant_intents: [...],
      distilled_wisdom: "...",
      abstract_plan_template: "...",
      token_usage: { intents: 1200, wisdom: 400, plan: 300 }
    }
```

---

## Subconscious: Background Intelligence

From [subconscious.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/subconscious.rs):

```rust
pub struct SubconsciousV1 {
    analysis_engine: AnalysisEngine,
    optimization_engine: OptimizationEngine,
    pattern_recognizer: PatternRecognizer,
}
```

**Purpose**: Continuous background processing of CausalChain to:
- Detect recurring failure patterns (using `learning.extract_patterns` logic)
- Identify optimization opportunities
- Recognize successful capability compositions
- Pre-compute wisdom for Context Horizon

**Maps to**: `ccos/src/learning/capabilities.rs` (pre-existing learning logic)

### MCP Exposure

```json
ccos/get_failure_patterns
  - input: { domains: ["github", "planning"], since_s: 86400 }
  - output: {
      patterns: [
        { signature: "tool:github/list error:403", count: 12, context: "missing_token" },
        { signature: "plan:split_file type_error", count: 5, context: "expected_string_got_null" }
      ],
      suggested_mitigations: ["Check auth token existence before call"]
    }

**Maps to**: `learning.get_failures` and `learning.extract_patterns` in [learning/capabilities.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/learning/capabilities.rs)

ccos/trigger_background_analysis
  - input: { focus_areas: ["recent_failures"] }
  - output: { analysis_job_id }
```

---

## Isolation & Security

### The Security Model

CCOS synthesizes code at runtime. This requires:

1. **Effect Declarations**: Every capability declares its effects
   ```clojure
   (defcap "my-capability"
     :effects [:io :read] [:network :get]
     ...)
   ```

2. **Constitution Enforcement**: GovernanceKernel validates effects match policy

3. **Sandbox Execution**: Synthesized RTFS runs in isolated context
   - No access to host filesystem
   - Network calls only to approved endpoints
   - Resource limits (time, memory)

4. **Purity Validation**: For adapters/transformers, ensure no side effects
   ```rust
   // From governance_kernel.rs L355-391
   pub fn validate_purity(&self, rtfs_code: &str) -> RuntimeResult<()>
   ```

### MCP Exposure

```json
ccos/execute_isolated
  - input: {
      rtfs_code: "...",
      allowed_effects: ["io.read", "network.get"],
      timeout_ms: 5000,
      memory_limit_mb: 128
    }
  - output: {
      result: {...},
      actual_effects: ["io.read"],
      resource_usage: { time_ms: 230, memory_mb: 12 }
    }

ccos/sandbox_test
  - input: { rtfs_code, mock_inputs }
  - output: { success, outputs, effects_would_trigger }
```

---

## Auto-Repair & Grounded Execution

### The Problem: LLM-Generated Code Fails

Generated RTFS code can fail in multiple ways:
1. **Parse errors** — Syntax mistakes (missing parentheses, malformed expressions)
2. **Type errors** — Schema mismatches detected by RTFS compiler
3. **Runtime errors** — Capability not found, unexpected data shape, API errors
4. **Semantic errors** — Code runs but doesn't match intent

Traditional approach: fail and ask user to fix. CCOS approach: **auto-repair with LLM feedback**.

### The Repair Loop

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Auto-Repair Architecture                             │
│                                                                             │
│   Generate          Compile/Execute          Error?           Repair        │
│   ────────          ───────────────          ──────           ──────        │
│                                                                             │
│   LLM produces      RTFS compiler        ┌─► Yes ──► LLM receives:         │
│   RTFS plan    ───► validates syntax ───►│           • Original code       │
│                     + types              │           • Error message        │
│                                          │           • RTFS grammar hints   │
│                     Executor runs        │           • Schema constraints   │
│                     step-by-step         │                                  │
│                                          │           LLM produces fix       │
│                                          │                   │              │
│                                          └─► No ───► Done    ▼              │
│                                                      ◄───── Loop (max N)    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### MCP Tools for Repair

```json
ccos/compile_rtfs
  - input: { rtfs_code }
  - output: {
      success: boolean,
      errors: [{ line, column, message, suggestion }],
      grammar_hint: "Expected: (let [binding expr] body) | (call capability params)",
      schema_context: { available_capabilities: [...], type_signatures: {...} }
    }

ccos/repair_rtfs
  - input: {
      rtfs_code: "...",
      error: { type: "parse" | "type" | "runtime", message: "..." },
      grammar_hints: true,
      max_attempts: 3
    }
  - output: {
      repaired_code: "...",
      repair_explanation: "Changed parameter name from 'state' to 'status'",
      confidence: 0.85
    }

ccos/explain_error
  - input: { error, rtfs_code }
  - output: {
      explanation: "Human-readable explanation of what went wrong",
      likely_causes: [...],
      suggested_fixes: [...]
    }

ccos/apply_fix
  - input: { capability_id, error_category, action: "retry" | "adjust_timeout" }
  - output: { success, remediation_action, plan_modifications: [...] }
```

**Maps to**: `llm_repair_runtime_error` in [validation.rs] and `learning.apply_fix` in [learning/capabilities.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/learning/capabilities.rs)

### Grammar Hints for LLM Repair

When repair is needed, CCOS provides **RTFS grammar hints** to guide the LLM:

```
RTFS Grammar Reference:
- (let [bindings...] body) — Local bindings
- (call "capability-id" {:param value}) — Capability invocation
- (if condition then else) — Conditional
- (fn [args] body) — Lambda/anonymous function
- (map fn collection) — Transform collection
- (filter pred collection) — Filter collection
- (get map :key) — Extract value from map
- (get map :key default) — Extract with default
- {:key value ...} — Map literal
- [:item1 :item2] — Vector literal

Common Errors:
- "expected map, got vector" → Use (get result :field) to extract from response
- "undefined symbol" → Only use RTFS stdlib, not invented functions
- "expected vector with keyword" → Use {:state "open"} not [:state "open"]
```

### Grounded Progressive Execution

#### The Concept

Instead of generating a complete plan upfront, **generate and execute step-by-step**:

1. Generate Step 1
2. Execute Step 1 (if safe)
3. Use Step 1 result to **ground** Step 2 generation
4. Generate Step 2 (with real data from Step 1)
5. Execute Step 2
6. ... continue

This prevents hallucination because each step is grounded in **actual execution results**.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     Grounded Progressive Execution                           │
│                                                                             │
│   Step 1                 Step 2                 Step 3                      │
│   ──────                 ──────                 ──────                      │
│                                                                             │
│   Goal: "List issues"    Goal: "Group by label" Goal: "Summarize each"     │
│         ↓                      ↓                      ↓                    │
│   Generate:              Generate:              Generate:                   │
│   (call list_issues)     (group-by             (map summarize               │
│         │                  (get step_1 :items)   grouped_items)             │
│         ▼                  :label)                    │                    │
│   Execute → Result:            │                      ▼                    │
│   [{:id 1, :labels      ──────►│                 Execute with               │
│     ["bug"]}...]               ▼                 real grouped data          │
│         │                Execute with                                       │
│         │                real issue data                                    │
│         │                      │                                            │
│         └──────────────────────┴─────────── Grounded (no hallucination) ───┘
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### When to use: Batch vs. Grounded

| Scenario | Recommended Approach | Why? |
|----------|----------------------|------|
| **Deterministic Tasks** | `generate_plan` + `execute_plan` | Faster, lower latency, atomic execution. <br>Ex: "Delete these 5 specific files" |
| **Exploratory Tasks** | `execute_step` (Grounded) | Next steps depend on hidden state. <br>Ex: "Debug why the build failed" |
| **High Ambiguity** | `execute_step` (Grounded) | Allows human correction at each step. |
| **Known Workflows** | `generate_plan` | Plan is already proven/archived. |

#### MCP Tools for Grounded Execution

```json
ccos/execute_step
  - input: {
      step_rtfs: "...",
      previous_results: { step_1: {...}, step_2: {...} },
      governance_check: true
    }
  - output: {
      result: {...},
      step_id: "step_3",
      can_proceed: true,
      next_step_hints: { available_data: [...], suggested_operations: [...] }
    }

ccos/generate_next_step
  - input: {
      goal: "...",
      completed_steps: [{ id: "step_1", result: {...} }, ...],
      remaining_intents: [...]
    }
  - output: {
      step_rtfs: "...",
      reasoning: "Based on step_1 result, we now have issues to group",
      is_final_step: false
    }

ccos/execute_grounded_plan
  - input: {
      goal: "...",
      strategy: "progressive" | "batch",
      pause_on_ambiguity: true
    }
  - output: {
      execution_trace: [...steps with results...],
      final_result: {...},
      adaptations_made: ["Changed step 3 after seeing step 2 result"]
    }
```

### Plan Adaptation During Execution

When an intermediate result differs from expectations:

1. **Detect divergence** — Actual data shape ≠ expected
2. **Pause execution** — Don't blindly continue
3. **Adapt plan** — Regenerate remaining steps with new information
4. **Optionally consult governance** — If adaptation is significant

```json
ccos/adapt_plan
  - input: {
      original_plan: "...",
      executed_steps: [...],
      divergence: { step: "step_2", expected: {...}, actual: {...} }
    }
  - output: {
      adapted_plan: "...",
      changes: ["Removed step 3, added new step for empty result handling"],
      requires_approval: false
    }
```

### Chat Agent Awareness

The chat agent (Claude, Gemini, etc.) should be **informed** about these capabilities via system prompt or tool descriptions:

```
CCOS Capabilities for the Chat Agent:

1. **Auto-Repair Available**: When you generate RTFS that fails, you can call:
   - ccos/compile_rtfs to check syntax before execution
   - ccos/repair_rtfs to get LLM-assisted fixes with grammar hints
   - ccos/explain_error to understand what went wrong

2. **Grounded Execution**: You don't have to generate complete plans:
   - Call ccos/execute_step for each step, using real results
   - Call ccos/generate_next_step to get next step based on actual data
   - The system prevents hallucination by grounding in execution

3. **Adaptive Planning**: Plans can change mid-execution:
   - If step results differ from expectations, call ccos/adapt_plan
   - The system will regenerate remaining steps with real context
   - You'll be notified of adaptations for transparency

4. **Repair Loop Contract**:
   - Maximum 3 repair attempts per error
   - Each attempt gets fresh grammar hints and error context
   - If repair fails, escalate to user with ccos/explain_error

5. **Progressive Strategy**: For complex goals, prefer:
   - analyze_goal → resolve step 1 → execute_step → resolve step 2 → ...
   - Over: analyze_goal → generate_full_plan → execute_plan
   - This grounds each step in reality
```

### Example: Grounded Repair Flow

```
User: "Group my GitHub issues by priority"

Agent:
1. ccos/analyze_goal → intents: [list_issues, group_by_priority]

2. ccos/generate_plan → RTFS:
   (let [issues (call "mcp.github/list_issues" {:state "open"})]
     (group-by (fn [i] (get i :priority)) issues))

3. ccos/execute_plan → Error: "no :priority field in issue"

4. ccos/explain_error → "GitHub issues use 'labels' array, not 'priority' field"

5. ccos/repair_rtfs → Repaired:
   (let [issues (call "mcp.github/list_issues" {:state "open"})]
     (group-by (fn [i] 
       (let [labels (get i :labels [])]
         (if (some #(= (get % :name) "priority:high") labels)
           "high"
           (if (some #(= (get % :name) "priority:low") labels)
             "low"
             "normal"))))
       issues))

6. ccos/execute_plan → Success! Returns grouped issues
```

---

## RTFS: Beyond JSON

### Why RTFS Matters

| Feature | JSON (MCP) | RTFS |
|---------|------------|------|
| Data representation | ✅ Yes | ✅ Yes |
| Executable logic | ❌ No | ✅ Yes |
| Function composition | ❌ No | ✅ Yes |
| LLM-friendly syntax | ✅ Yes | ✅ Yes (S-expressions) |
| Type schemas | ✅ JSON Schema | ✅ TypeExpr |
| Effect declarations | ❌ No | ✅ Yes |
| Self-modifying | ❌ No | ✅ Code is data |

### RTFS as the Agent's Native Language

When a chat agent asks CCOS to synthesize a capability:
- The result is **RTFS code**, not a tool description
- RTFS can be **inspected** by the agent
- RTFS can be **modified** by the agent before registration
- RTFS **composes** with other RTFS

```clojure
;; Agent synthesizes a grouping function
(defcap "generated/group_by_priority"
  :input-schema {:items [:vector :any]}
  :output-schema [:map :string [:vector :any]]
  :effects []
  (fn [{:keys [items]}]
    (group-by #(get % :priority "none") items)))

;; Agent composes it with existing capability
(defcap "generated/summarize_by_priority"
  :input-schema {:issues [:vector :any]}
  :effects [:network :get]
  (fn [{:keys [issues]}]
    (let [grouped (call "generated/group_by_priority" {:items issues})]
      (map (fn [[priority items]]
             {:priority priority
              :count (count items)
              :summary (call "llm/summarize" {:text (str items)})})
           grouped))))
```

---

## Complete Tool Landscape

### Static Tools (Always Available)

| Category | Tool | Description |
|----------|------|-------------|
| **Discovery** | `ccos/discover_capabilities` | Search for tools by query/domain |
| | `ccos/search_tools` | Full-text capability search |
| | `ccos/introspect_server` | MCP introspection on external server |
| | `ccos/register_server` | Add new MCP server |
| **Planning** | `ccos/analyze_goal` | Decompose goal into intents |
| | `ccos/resolve_intent` | Find capability for intent |
| | `ccos/generate_plan` | Create RTFS plan |
| | `ccos/validate_plan` | Governance + static checks |
| **Execution** | `ccos/execute_plan` | Run plan (with streaming) |
| | `ccos/execute_isolated` | Sandboxed execution |
| | `ccos/cancel_execution` | Abort running plan |
| **Synthesis** | `ccos/synthesize_capability` | Create new capability |
| | `ccos/synthesize_strategy` | Create new planning strategy |
| | `ccos/synthesize_from_error` | Auto-fix from failure |
| **Repair** | `ccos/compile_rtfs` | Validate RTFS syntax and types |
| | `ccos/repair_rtfs` | LLM-assisted code repair with grammar hints |
| | `ccos/explain_error` | Human-readable error explanation |
| | `ccos/apply_fix` | Apply automated heuristics (retry, timeout) |
| **Agent Context** | `ccos/log_thought` | Record agent reasoning for learning |
| **Grounded Execution** | `ccos/execute_step` | Execute single step with previous results |
| | `ccos/generate_next_step` | Generate next step based on actual data |
| | `ccos/execute_grounded_plan` | Progressive step-by-step execution |
| | `ccos/adapt_plan` | Adapt plan based on intermediate results |
| **Memory** | `ccos/query_memory` | Search WorkingMemory |
| | `ccos/learn_from_execution` | Log outcome for learning |
| | `ccos/find_similar_plans` | Find past similar goals |
| | `ccos/archive_plan` | Save plan for reuse |
| | `ccos/replay_plan` | Re-execute archived plan |
| **Meta** | `ccos/meta_plan` | Get strategic planning advice |
| | `ccos/list_strategies` | Available planning strategies |
| | `ccos/evaluate_strategy` | Update strategy confidence |
| | `ccos/get_failure_patterns` | Background failure analysis results |
| **Governance** | `ccos/request_governance_decision` | Multi-party approval |
| | `ccos/provide_governance_response` | Submit decision |
| | `ccos/get_constitution` | View current rules |
| | `ccos/set_policy` | Update governance policy |
| **Inspection** | `ccos/get_causal_chain` | Query execution history |
| | `ccos/explain_plan` | Human-readable explanation |
| | `ccos/get_capability_graph` | Capability dependency graph |
| | `ccos/get_context_for_task` | Context-aware recall |

### Dynamic Tools (Generated at Runtime)

All synthesized capabilities become callable as:
- `ccos/generated/{capability_name}`

Example: After `ccos/synthesize_capability({ description: "group by priority" })`:
- New tool: `ccos/generated/group_by_priority`

---

## Over-Engineering Analysis

> **Critical Question**: Is this architecture over-engineered?

### Arguments FOR "Over-Engineered"

1. **Complexity Overhead**
   - CCOS requires Rust runtime, RTFS compiler, MCP server
   - Python + JSON uses existing ubiquitous infrastructure

2. **Learning Curve**
   - Developers must learn RTFS, CCOS concepts
   - Python is already known by most developers

3. **Immediate Utility**
   - For single-session tasks, Python scripts work fine
   - CCOS machinery adds latency for simple goals

4. **Ecosystem Lock-in**
   - Python scripts are portable
   - CCOS capabilities require CCOS runtime

5. **Development Speed**
   - LLMs generate Python well today
   - RTFS generation requires training/prompting

### Arguments AGAINST "Over-Engineered"

1. **The Memory Problem is Real**
   - Chat agents forget between sessions
   - CCOS persists knowledge across time

2. **Governance at Scale**
   - Enterprises can't auto-run arbitrary Python
   - CCOS provides auditable, governed execution

3. **Multi-Agent Coordination**
   - Multiple agents sharing Python scripts = chaos
   - CCOS provides shared semantic state

4. **Self-Improvement Loop**
   - Python scripts don't self-optimize
   - CCOS learns from failures, evolves strategies

5. **Effect Tracking**
   - Python can do anything (dangerous)
   - CCOS requires explicit effect declarations

6. **Composition**
   - Python scripts are monolithic
   - RTFS capabilities compose naturally

### The Verdict: **Context-Dependent**

```
┌────────────────────────────────────────────────────────────────────────────┐
│                    When to Use Each Approach                                │
│                                                                            │
│  Python + JSON                           CCOS + RTFS                       │
│  ─────────────                           ───────────                       │
│                                                                            │
│  • Developer using chat agent            • Autonomous agents              │
│  • One-off tasks                         • Persistent workflows           │
│  • Single session                        • Multi-session continuity       │
│  • Personal use                          • Enterprise governance          │
│  • Integration with Python libs          • Self-improving systems         │
│  • Quick prototyping                     • Multi-agent coordination       │
│  • Trusted environment                   • Untrusted code execution       │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### Recommendation

**Hybrid approach**: CCOS can also execute Python in sandboxed capabilities:

```clojure
(defcap "generated/pandas_analysis"
  :effects [:sandbox :python]
  :runtime :python
  (python-code "
    import pandas as pd
    df = pd.DataFrame(input['data'])
    return df.groupby('priority').agg({'count': 'sum'}).to_dict()
  "))
```

This gives:
- Python's ecosystem when needed
- CCOS's governance and memory
- Best of both worlds

---

## Open Questions

### 1. Auth Token Flow & Secret Management
How do synthesized tools access secrets (API keys, DB passwords)?
- **Current Restriction**: Synthesized code runs in a **strict sandbox** (see `security_policies.rs`). It has **no access** to the host process's environment variables (`env`) or filesystem unless explicitly allowlisted. This prevents malicious or buggy code from traversing `process.env` and exfiltrating `AWS_ACCESS_KEY` or other sensitive system secrets.
- **Proposed Solution**: Use the `ccos/request_secret` capability.
  - The tool explicitly requests `request_secret("STRIPE_API_KEY")`.
  - CCOS checks its internal vault (or prompts the user) and injects *only that specific value* into the capability's execution context.
  - This enforces the **Principle of Least Privilege**: tools get exactly the secrets they need, nothing more.
- **Risk**: Malicious synthesized code stealing secrets. Needs strict governance on `request_secret`.

### 2. Streaming Semantics
Should `execute_plan` stream results?
- **Yes**: For long-running plans, agent needs progress updates.
- **Protocol**: SSE (Server-Sent Events) over MCP.

### 3. Multi-Agent Conflicts
If two agents modify IntentGraph simultaneously:
- Optimistic concurrency with conflict detection?
- Versioned state with merge semantics?
- Agent-scoped namespaces?

### 4. RTFS vs JSON for Agent I/O
Should agents see raw RTFS or JSON representations?
- LLMs handle JSON better today
- RTFS enables logic exchange
- Possible: JSON for simple data, RTFS for logic

### 5. DialoguePlanner Migration
Keep `DialoguePlanner` as:
- Demo/testing mode?
- Fallback for offline use?
- Remove entirely after MCP migration?

### 6. Constitution Mutability
Can agents propose Constitution changes?
- Requires meta-governance
- Risk of agent self-modification
- Human-in-the-loop always for Constitution?

---

## Agent System Prompt Guidelines

To ensure the chat agent interacts effectively with CCOS, the following **Protocol** should be part of its system prompt:

### The CCOS Protocol

1.  **Discovery First**
    - **Rule**: Before assuming a tool is missing, ALWAYS call `ccos/search_tools`.
    - **Why**: The capability library grows dynamically. A tool that didn't exist yesterday might exist today.

2.  **Think Before Acting**
    - **Rule**: For any task with >1 step or high ambiguity, call `ccos/log_thought` BEFORE executing.
    - **Why**: This "saves your game." If you crash or fail, CCOS uses this thought log to help you recover next time.

3.  **Synthesize, Don't hallucinate**
    - **Rule**: If a capability is truly missing, DO NOT invent a Python script. Call `ccos/synthesize_capability`.
    - **Why**: Synthesized tools are persistent, governed, and reusable. One-off scripts are lost.

4.  **Ground Your Plans**
    - **Rule**: If you don't know the exact data shape, use **Grounded Execution** (`ccos/execute_step`).
    - **Why**: Prevents "hallucinating parameters" errors (e.g., guessing a field is named `id` when it's `issue_id`).

5.  **When in Doubt, Meta-Plan**
    - **Rule**: If a goal seems too complex, call `ccos/meta_plan` to ask CCOS for a strategy.
    - **Why**: CCOS remembers past successful strategies for similar goals.

---

## Implementation Roadmap

### Phase 1: Core MCP Server (Week 1-2)
- [ ] Create `ccos/src/mcp/planning_server.rs`
- [ ] Implement static tools: `analyze_goal`, `discover_capabilities`, `resolve_intent`
- [ ] Wire to existing `ModularPlanner` and `ResolutionStrategy`
- [ ] Test with Claude Desktop MCP client

### Phase 2: Synthesis & Execution (Week 3-4)
- [ ] Implement `synthesize_capability`, `generate_plan`
- [ ] Implement `validate_plan`, `execute_plan` with streaming
- [ ] Add dynamic tool registration
- [ ] Expose resource endpoints for state inspection
- [ ] Implement `ccos/request_secret` (Security)
- [ ] Implement `ccos/log_thought` (Agent Context)

### Phase 3: Learning & Memory (Week 5-6)
- [ ] Implement `query_memory`, `learn_from_execution`
- [ ] Wire WorkingMemory to MCP resources
- [ ] Add `find_similar_plans`, `archive_plan`, `replay_plan`
- [ ] Expose `ccos/apply_fix` (Automated Heuristics)

### Phase 4: Meta-Planning & Governance (Week 7-8)
- [ ] Implement `meta_plan`, `synthesize_strategy`
- [ ] Implement `request_governance_decision`
- [ ] Build multi-party approval dialogue

### Phase 5: Subconscious & Full Loop (Week 9-10)
- [ ] Connect `subconscious.rs` to CausalChain events
- [ ] Implement `trigger_background_analysis`
- [ ] Verify full self-improvement loop (Fail -> Learn -> Fix)


## Next Steps

1. **Validate core assumptions** with minimal prototype
2. **Pick first tool subset** for implementation
3. **Design RTFS ↔ JSON serialization** for agent compatibility
4. **Define governance dialogue schema** for multi-party approval
5. **Create demo scenario** showing self-improvement loop

---

*This document is a living design. We expect significant iteration as we prototype and learn.*
