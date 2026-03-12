# Plan: Adapter Strategy — Replace Overlay System with Schema-Driven Adaptation

## Context

The current overlay system (`agent.adapt` + composition + pre/post hooks) adds ~1,220 lines across 14 files but solves a problem that can be handled better by an adapter specialist agent + I/O schemas. This plan replaces overlays with a simpler, agent-centric approach.

---

## Phase 1: Add I/O Schema Support to Agent Manifests

**Goal:** Make agent I/O contracts machine-readable so adaptation decisions become mechanical.

### 1.1 Add schema types to agent manifest

**File: `autonoetic-types/src/agent.rs`**
- Add `AgentIO` struct with `accepts: Option<serde_json::Value>` and `returns: Option<serde_json::Value>`
- Add `pub io: Option<AgentIO>` field to `AgentManifest`
- Both fields are JSON Schema values (following existing `ToolDefinition.input_schema` pattern in `llm/mod.rs:55`)

### 1.2 Parse schemas from SKILL.md frontmatter

**File: `autonoetic-gateway/src/runtime/parser.rs`**
- Parse `metadata.autonoetic.io.accepts` and `metadata.autonoetic.io.returns` from YAML frontmatter
- Default to `None` when not present (backward compatible)

### 1.3 Add I/O schemas to existing agent manifests

**Files to update (add `io` section to `metadata.autonoetic`):**

| Agent | accepts | returns |
|-------|---------|---------|
| `agents/specialists/researcher.default/SKILL.md` | `{query: string, domain?: string}` | `{findings: array, summary: string}` |
| `agents/specialists/coder.default/SKILL.md` | `{task: string, context?: string, constraints?: array}` | `{changes: array, verification: object, risks: array}` |
| `agents/specialists/architect.default/SKILL.md` | `{problem: string, constraints?: array, existing?: string}` | `{design: string, interfaces: array, tradeoffs: array}` |
| `agents/specialists/debugger.default/SKILL.md` | `{failure: string, logs?: string, expected?: string}` | `{root_cause: string, fix: string, evidence: array}` |
| `agents/specialists/evaluator.default/SKILL.md` | `{artifact: string, criteria: array, test_command?: string}` | `{passed: boolean, results: array, confidence: float}` |
| `agents/specialists/auditor.default/SKILL.md` | `{target: string, scope: array}` | `{findings: array, risk_level: string, recommendations: array}` |
| `agents/lead/planner.default/SKILL.md` | (user-facing, no strict schema) | `{mode: string, plan: array, result: string}` |
| `agents/evolution/specialized_builder.default/SKILL.md` | `{role: string, requirements: object, constraints?: array}` | `{agent_id: string, status: string}` |

### 1.4 Expose schemas in agent discovery

**File: `autonoetic-gateway/src/runtime/tools.rs`**
- Update `agent.discover` tool output to include `io` field from manifest
- Planner can now compare schemas mechanically

### 1.5 Add schema validation in execution

**File: `autonoetic-gateway/src/execution.rs`**
- In `spawn_agent_once()`, validate spawn input against target agent's `accepts` schema (if present)
- Log validation result to causal chain as informational event (do NOT hard-fail — LLM can handle minor mismatches)
- Add output validation in `lifecycle.rs`: log output against `returns` schema after execution

### 1.6 Tests

- Unit test: parser extracts I/O schemas from SKILL.md frontmatter
- Unit test: parser defaults to None when schema absent
- Unit test: `agent.discover` returns schema in output
- Integration test: spawn with mismatched input logs warning to causal chain
- Integration test: spawn with valid input passes schema check

---

## Phase 2: Add Middleware Support to Agent Manifests

**Goal:** Move pre/post hooks from overlay JSON to agent-owned declarations. Agents remain immutable; middleware is part of the agent definition.

### 2.1 Add middleware types

**File: `autonoetic-types/src/agent.rs`**
- Rename `AdaptationHooks` to `Middleware` (keep same structure: `pre_process: Option<String>`, `post_process: Option<String>`)
- Move `middleware: Option<Middleware>` to `AgentManifest` (replacing `adaptation_hooks`)
- Remove `AdaptationHooks`, `AssetChange`, `AssetAction` types (only used by overlay system)

### 2.2 Parse middleware from SKILL.md

**File: `autonoetic-gateway/src/runtime/parser.rs`**
- Parse `metadata.autonoetic.middleware.pre_process` and `post_process` from frontmatter
- Replace `adaptation_hooks: None` with `middleware: None`

### 2.3 Simplify hook execution in lifecycle

**File: `autonoetic-gateway/src/runtime/lifecycle.rs`**
- Replace `adaptation_hooks: AdaptationHooks` field with `middleware: Middleware`
- Replace `with_adaptation_hooks()` with `with_middleware()`
- Remove `with_adaptation_assets()` — no longer needed
- Remove `project_adaptation_assets()` — no longer needed
- Keep `apply_pre_process_hook()`, `apply_post_process_hook()`, `run_hook_sandbox()` — rename to `apply_middleware_pre()`, `apply_middleware_post()`, `run_middleware_script()`
- Middleware scripts are resolved relative to the agent's own directory (not projected)

### 2.4 Update execution path

**File: `autonoetic-gateway/src/execution.rs`**
- Replace `with_adaptation_hooks(loaded.adaptation_hooks)` with `with_middleware(loaded.manifest.middleware.clone())`
- Remove `extract_selected_adaptation_ids()` function
- Remove `adaptation_assets` from executor construction

### 2.5 Update agent loading

**File: `autonoetic-gateway/src/agent/repository.rs`**
- Remove `get_sync_with_adaptations()` method
- Remove `compose_instructions_with_adaptations()` function (~170 lines)
- Remove `AdaptationOverlay`, `AdaptationComposition` structs
- Remove `adaptation_hooks` and `adaptation_assets` from `LoadedAgent`
- Simplify `LoadedAgent` to `{ dir, manifest, instructions }`
- Keep `get_sync()` as the sole loading method

### 2.6 Tests

- Unit test: parser extracts middleware from SKILL.md
- Unit test: middleware pre_process hook runs and transforms input
- Unit test: middleware post_process hook runs and transforms output
- Integration test: agent with middleware in manifest executes hooks correctly

---

## Phase 3: Create Adapter Specialist Agent

**Goal:** Replace `agent.adapt` tool with a specialist that generates wrapper agents.

### 3.1 Create adapter specialist bundle

**New directory: `agents/evolution/agent-adapter.default/`**
- `SKILL.md`: Instructions for reading base agent manifest + target requirements, generating wrapper agent
- `runtime.lock`: Standard runtime lock

**Adapter specialist workflow:**
1. Receive: `base_agent_id`, `target_spec` (desired I/O schema + behavior modifications), `rationale`
2. Read base agent's SKILL.md + manifest (including I/O schemas)
3. Compare base `accepts`/`returns` schemas vs target spec
4. Generate wrapper agent:
   - New `SKILL.md` with adapted instructions + mapping middleware
   - Pre-process script for input mapping (if schemas differ)
   - Post-process script for output mapping (if schemas differ)
5. Register via `agent.install`
6. Return new agent ID to caller

### 3.2 Add I/O mapping capabilities

**New files in adapter specialist's `scripts/`:**
- `schema_diff.py`: Compares two JSON schemas and generates mapping description
- `generate_wrapper.py`: Takes base SKILL.md + diff → produces adapted SKILL.md + middleware scripts

### 3.3 Register in role catalog

**File: `agents/lead/planner.default/SKILL.md`**
- Add `agent-adapter` → `agent-adapter.default` to role registry

### 3.4 Tests

- Unit test: adapter specialist generates wrapper with correct schema mapping
- Integration test: base agent + adapter specialist → wrapper agent → execution produces correct I/O transformation
- Integration test: wrapper agent inherits base capabilities correctly

---

## Phase 4: Update Planner and Builder Instructions

**Goal:** Replace overlay-based adaptation flow with adapter specialist delegation.

### 4.1 Update planner.default SKILL.md

**File: `agents/lead/planner.default/SKILL.md`**

Replace lines 83-88 (reuse-first decision ladder):

```
## Reuse-First Decision Ladder
1. Call `agent.discover` with required intent and capabilities
2. If strong match (schema compatible, fitness score > 20), spawn as-is
3. If moderate match (schemas incompatible or partial fit), delegate to `agent-adapter.default`
   - Provide: base_agent_id, target I/O spec, behavior gap
   - Agent-adapter generates a wrapper agent
   - Spawn the wrapper agent for this and future requests
4. If no match, delegate to `specialized_builder.default` to create new specialist
```

Remove references to:
- `agent.adapt` (lines 86, 88)
- `adaptation_hooks` (line 88)
- `selected_adaptation_ids`

### 4.2 Update specialized_builder.default SKILL.md

**File: `agents/evolution/specialized_builder.default/SKILL.md`**

Replace lines 47-48 (adaptation references):
- Remove "prefer `agent.adapt` over replacement"
- Add "if role exists with minor gap, suggest delegating to `agent-adapter.default`"

### 4.3 Update foundation rules

**File: `autonoetic-gateway/src/runtime/foundation.rs`** (or wherever foundation instructions are assembled)
- Remove any references to overlay system, `selected_adaptation_ids`
- No replacement needed — middleware is automatic when present in manifest

---

## Phase 5: Remove Overlay System

**Goal:** Delete all overlay/adaptation infrastructure.

### 5.1 Remove `agent.adapt` tool

**File: `autonoetic-gateway/src/runtime/tools.rs`**
- Delete `AgentAdaptArgs` struct (lines 3195-3211)
- Delete `PromotionGate` struct (lines 3213-3221)
- Delete `AdaptationMetadata` struct (lines 3223-3231)
- Delete `AgentAdaptTool` struct and impl (lines 3233-3455, ~220 lines)
- Remove `registry.register(Box::new(AgentAdaptTool))` (line 3473)
- Delete adaptation-related unit tests (lines 5414-5829, ~415 lines)

### 5.2 Remove overlay types

**File: `autonoetic-types/src/agent.rs`**
- Delete `AdaptationHooks` struct
- Delete `AssetAction` enum
- Delete `AssetChange` struct

### 5.3 Remove overlay composition logic

**File: `autonoetic-gateway/src/agent/repository.rs`**
- Delete `AdaptationOverlay` struct
- Delete `AdaptationComposition` struct
- Delete `get_sync_with_adaptations()` method
- Delete `load_from_meta_with_adaptations()` method
- Delete `compose_instructions_with_adaptations()` function (~170 lines)
- Remove `adaptation_hooks` and `adaptation_assets` from `LoadedAgent`
- Delete adaptation-related unit tests (lines 616-726)

### 5.4 Remove overlay entry points

**File: `autonoetic-gateway/src/execution.rs`**
- Delete `extract_selected_adaptation_ids()` function
- Remove `selected_adaptation_ids` extraction from `spawn_agent_once()`
- Remove `with_adaptation_hooks()` and `with_adaptation_assets()` calls

### 5.5 Remove hook projection

**File: `autonoetic-gateway/src/runtime/lifecycle.rs`**
- Delete `with_adaptation_assets()` method
- Delete `project_adaptation_assets()` method (~45 lines)

### 5.6 Remove overlay storage directory

**Directory: `agents/.gateway/adaptations/`**
- Delete entire directory tree

### 5.7 Delete overlay test files

- Delete `autonoetic-gateway/tests/adaptation_composition_integration.rs` (145 lines)
- Delete `autonoetic-gateway/tests/pipeline_hooks_integration.rs` (228 lines)

### 5.8 Update defaults in non-core files

**Files with `adaptation_hooks: None` defaults:**
- `autonoetic-gateway/src/runtime/parser.rs:103` — replace with `middleware: None`
- `autonoetic-gateway/src/policy.rs:243` — replace with `middleware: None`
- `autonoetic-gateway/src/runtime/tools.rs:2708, 3516` — replace with `middleware: None`

### 5.9 Delete documentation

- Delete `docs/adaptation-composition-model.md` (219 lines)
- Update `plan.md`: remove adaptation feature checklist items

---

## Phase 6: Add Middleware to Existing Agents (Optional)

**Goal:** Demonstrate the middleware pattern on agents that need deterministic data transformation.

### 6.1 Example: Input normalization for researcher.default

Create `agents/specialists/researcher.default/scripts/normalize_query.py`:
- Strips whitespace, normalizes encoding
- Add to `SKILL.md` frontmatter: `middleware: { pre_process: "scripts/normalize_query.py" }`

### 6.2 Example: Output formatting for coder.default

Create `agents/specialists/coder.default/scripts/format_output.py`:
- Ensures output matches declared `returns` schema
- Add to `SKILL.md` frontmatter: `middleware: { post_process: "scripts/format_output.py" }`

---

## File Change Summary

| Action | File | Lines affected |
|--------|------|---------------|
| **Modify** | `autonoetic-types/src/agent.rs` | ~40 changed, add `AgentIO`, `Middleware`; remove `AdaptationHooks`, `AssetChange`, `AssetAction` |
| **Modify** | `autonoetic-gateway/src/runtime/parser.rs` | ~5 changed, parse `io` + `middleware` |
| **Modify** | `autonoetic-gateway/src/agent/repository.rs` | ~250 removed (composition logic), simplify `LoadedAgent` |
| **Modify** | `autonoetic-gateway/src/execution.rs` | ~50 removed (overlay entry points) |
| **Modify** | `autonoetic-gateway/src/runtime/lifecycle.rs` | ~150 changed (rename hooks→middleware, remove projection) |
| **Modify** | `autonoetic-gateway/src/runtime/tools.rs` | ~635 removed (`agent.adapt` + tests) |
| **Modify** | `autonoetic-gateway/src/policy.rs` | ~1 line |
| **Delete** | `autonoetic-gateway/tests/adaptation_composition_integration.rs` | 145 lines |
| **Delete** | `autonoetic-gateway/tests/pipeline_hooks_integration.rs` | 228 lines |
| **Delete** | `docs/adaptation-composition-model.md` | 219 lines |
| **Create** | `agents/evolution/agent-adapter.default/SKILL.md` | ~100 lines |
| **Create** | `agents/evolution/agent-adapter.default/runtime.lock` | ~20 lines |
| **Create** | `agents/evolution/agent-adapter.default/scripts/schema_diff.py` | ~80 lines |
| **Create** | `agents/evolution/agent-adapter.default/scripts/generate_wrapper.py` | ~120 lines |
| **Modify** | 8 agent SKILL.md files | ~15 lines each (add `io` schemas) |
| **Modify** | `agents/lead/planner.default/SKILL.md` | ~10 lines changed |
| **Modify** | `agents/evolution/specialized_builder.default/SKILL.md` | ~5 lines changed |

**Net result:** Remove ~1,220 lines of overlay infrastructure, add ~500 lines of schema support + adapter specialist. Net reduction of ~720 lines, plus significantly simpler architecture.

---

## Execution Order

1. Phase 1 (schemas) — no breaking changes, purely additive
2. Phase 2 (middleware) — replaces overlay hooks with manifest-based hooks
3. Phase 3 (adapter specialist) — new agent bundle
4. Phase 4 (planner updates) — instruction changes only
5. Phase 5 (remove overlay) — breaking change, but planner already uses new flow
6. Phase 6 (optional examples) — demonstrate pattern

Phases 1-2 can be done together. Phase 5 must come after Phase 4. Phase 3 and 4 can be done together.
