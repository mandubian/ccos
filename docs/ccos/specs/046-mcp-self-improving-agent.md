# CCOS as a Self-Improving MCP Cognitive Substrate

> **Status**: Implemented
> **Date**: 2026-01-12
> **Author**: AI-Assisted Implementation

---

## Executive Summary

This document describes the transformation of CCOS from a monolithic planner into a **dynamic MCP server** where all CCOS features become callable tools for external chat agents. The architecture enables:

1. **Dynamic capability synthesis** — New capabilities are generated and served as MCP tools at runtime
2. **Tangible Learning** — The system explicitly records and recalls patterns via `AgentMemory`
3. **Constitution Discovery** — Agents voluntarily discover and adhere to system rules
4. **RTFS as native language** — Logic and data exchange in a unified, LLM-friendly format

---

## Vision & Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          External Chat Agent                                 │
│               (Claude, Gemini, IDE Agent, Custom LLM)                       │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  "Agent Brain" / Dialogue Controller                                  │  │
│  │  - Decides when to plan, execute, learn                               │  │
│  │  - Queries CCOS for Constitution & Guidelines                         │  │
│  │  - Records thoughts and learnings explicitly                          │  │
│  └────────────────────────────────┬──────────────────────────────────────┘  │
└───────────────────────────────────┼─────────────────────────────────────────┘
                                    │ MCP Protocol (stdio/SSE)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CCOS MCP Server (Dynamic)                           │
│                                                                             │
│  ┌─────────────────┐  ┌────────────────┐  ┌────────────────────────────┐   │
│  │  Session Tools  │  │  Logic Tools   │  │  Learning Tools            │   │
│  │                 │  │                │  │                            │   │
│  │ session_start   │  │ ccos_plan      │  │ log_thought                │   │
│  │ execute_cap     │  │ suggest_apis   │  │ record_learning            │   │
│  │ session_plan    │  │ decompose      │  │ recall_memories            │   │
│  │ session_end     │  │ synthesize     │  │ consolidate_session        │   │
│  │                 │  │                │  │ get_constitution           │   │
│  └─────────────────┘  └────────────────┘  └────────────────────────────┘   │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │                    CCOS Core Engine (Rust)                            │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐  │  │
│  │  │ Modular     │ │ Intent      │ │ Capability  │ │ Governance      │  │  │
│  │  │ Planner     │ │ Graph       │ │ Marketplace │ │ Kernel          │  │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────┘  │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐                      │  │
│  │  │ Plan        │ │ Causal      │ │ Agent       │                      │  │
│  │  │ Archive     │ │ Chain       │ │ Memory      │                      │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘                      │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Insight

The **Dialogue Planner** role is externalized to the chat agent. CCOS provides the **Cognitive Primitives**:
- **Execution**: `ccos_execute_capability` handles the complexities of RTFS runtime.
- **Learning**: `ccos_record_learning` and `ccos_consolidate_session` turn experiences into persistent assets.
- **Reference**: `ccos_get_constitution` and `ccos_get_guidelines` provide ground truth.

---

## Core Subsystems as MCP

### Phase 1: Context & Planning

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

#### `ccos_search`
Search for available tools matching a query or domain.

```json
{
  "name": "ccos_search",
  "inputSchema": {
    "query": "string (required)",
    "domains": "string[] (optional)",
    "limit": "integer"
  }
}
```

#### `ccos_get_guidelines`
Get the official agent guidelines. The "Instruction Manual" for interacting with CCOS.

```json
{
  "name": "ccos_get_guidelines",
  "inputSchema": {}
}
```

---

### Phase 2: Learning & Memory ("Tangible Learning")

CCOS implements **Tangible Learning**, where memory is not just a vector store but a structured, queryable asset class.

#### `ccos_log_thought`
Record the agent's reasoning process. Used for immediate context and failure analysis.

```json
{
  "name": "ccos_log_thought",
  "inputSchema": {
    "thought": "string (required)",
    "plan_id": "string",
    "is_failure": "boolean"
  }
}
```

#### `ccos_record_learning`
Explicitly record a learned pattern. This is how the agent "teaches" its future self.

```json
{
  "name": "ccos_record_learning",
  "inputSchema": {
    "pattern": "string (required)",
    "context": "string (required)",
    "outcome": "string (required)",
    "confidence": "number"
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
    "limit": "integer"
  }
}
```

#### `ccos_consolidate_session`
**The Ultimate Learning Tool**. Converts an entire execution session (trace) into a reusable **Agent Capability**.

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

---

### Phase 3: Governance as Discovery

Instead of a blocking "Request Governance" tool, CCOS follows a **Discovery Model**. Agents are expected to read the Constitution and self-regulate, while critical actions are enforced by the Kernel.

#### `ccos_get_constitution`
Get the system constitution rules and policies.

```json
{
  "name": "ccos_get_constitution",
  "inputSchema": {}
}
```

**Returns**:
```json
{
  "constitution": {
    "rules": [
      {
        "id": "require-human-approval",
        "match_pattern": "ccos.cli.config.*",
        "action": "RequireGuardianApproval"
      },
      ...
    ]
  }
}
```

---

## Dynamic Capability Synthesis

The radical idea: **CCOS's MCP tool list grows at runtime**.

1. **Discovery**: Agent fails to find a tool for "group by label".
2. **Synthesis**: Agent calls `ccos_synthesize_capability` with description.
3. **Execution**: CCOS generates RTFS code.
4. **Registration**: The new capability is registered in the marketplace.
5. **Usage**: Agent calls `ccos_execute_capability` with the new ID.
6. **Consolidation**: After a successful session, agent calls `ccos_consolidate_session` to bake the entire workflow into a high-level agent.

### Example Flow

```
User: "Summarize my GitHub issues by priority"

Agent:
1. Calls ccos_plan -> intents: [list_issues, categorize_by_priority, summarize]
2. Calls ccos_decompose("categorize by priority") -> "missing"
3. Calls ccos_synthesize_capability({ description: "Group items by a priority field" })
   -> Returns: { capability_id: "synthesized.group_by_priority" }
4. Calls ccos_session_start
5. Calls ccos_execute_capability("mcp.github/list_issues")
6. Calls ccos_execute_capability("synthesized.group_by_priority", { ... })
7. Calls ccos_session_end
8. Returns summarized result to user.
```

---

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
