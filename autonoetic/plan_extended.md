# Autonoetic Gateway Simplification Plan

## Goal

Strip the gateway of domain-specific orchestration logic (roles, wake predicates, implicit routing, disclosure complexity, evolution gates) and replace it with **generic primitives** that agents compose into behaviors. The gateway becomes a dumb secure pipe. Agents do the reasoning.

## Guiding Principle

**The gateway provides primitives. Agents compose them into behaviors. Autonomy emerges from composition, not from platform rules.**

If the gateway has a concept like "lead agent", "specialist", "stale goal", "promotion gate", or "evolution steward" — it's coupling infrastructure to a specific orchestration pattern. Remove it.

## Convention, Not Mandate

After simplification, agents like the planner become the **default input agent by convention** — the sensible default that human-facing adapters point to. But this is a **deployment choice**, not a platform rule.

- The gateway never knows what a "planner" is
- Each adapter (CLI, WhatsApp, webhook) configures its own `default_agent`
- Any agent can be targeted directly by any caller
- The planner is the `index.html` of the system: a sensible default, not an enforced one

This means a "support bot" adapter can default to `support-agent.default`, a "code review" webhook can default to `auditor.default`, and a monitoring alert system can default to `debugger.default` — all without the gateway knowing or caring about any of them.

---

## Current State vs Target State

This section maps each simplification area to the existing implementation with exact file pointers, so changes can be traced from current code to target design.

| Area | Current State | Target State | Key Files |
|------|--------------|--------------|-----------|
| **Ingress routing** | 3-tier fallback: explicit target → session lead binding (persisted to disk) → `config.default_lead_agent_id` (`"planner.default"`). `resolve_ingest_target_agent_id()` at `router.rs:460-486`. Session bindings written by `persist_session_lead_agent_binding()` at `router.rs:596-616`. | Explicit `target_agent_id` required on all ingress. Per-adapter `default_agent` convention (CLI defaults to `planner.default`, webhook to `auditor.default`, etc.). Gateway never resolves a missing target. | `autonoetic-gateway/src/router.rs:460-616`, `autonoetic-types/src/config.rs:77` |
| **Wake predicates** | 7 `WakeReason` variants (`Timer`, `NewMessage`, `TaskCompletion`, `QueuedWork`, `StaleGoal`, `RetryableFailure`, `ApprovalResolved`) in `background.rs:177-185`. `should_wake()` is 203 lines in `decision.rs` with predicate cascade (approval resolution, pending actions, new messages, task completions, stale goals, retryable failures, general timer). | 2 variants: `Timer` and `Signal { name, payload }`. `should_wake()` ≤50 lines: check timer → check inbox signals → return None. Agents decide what "stale" or "retryable" means in their SKILL.md. | `autonoetic-types/src/background.rs:177-185`, `autonoetic-gateway/src/scheduler/decision.rs` (203 lines) |
| **Disclosure policy** | 4-class system (`Public`, `Internal`, `Confidential`, `Secret`) in `disclosure.rs`. `DisclosureState` at `runtime/disclosure.rs` (210 lines) with source-based classification, path-pattern matching, `evaluate_class()` rule chain. 7+ source categories (`user_input`, `session_context`, `tier1_memory`, `tier2_memory`, `secret_store`, `sandbox_output`, `external_tool`). | Binary `Safe`/`Restricted`. Tools declare `restricted_output: bool`. ~60 lines total. No source categories, no path-pattern matching. Simple taint tracking + reply filtering remains. | `autonoetic-types/src/disclosure.rs` (49 lines), `autonoetic-gateway/src/runtime/disclosure.rs` (210 lines) |
| **Evolution gates** | `requires_promotion_gate()` at `tools.rs:84` hardcodes `specialized_builder.default` and `evolution-steward.default`. `is_install_high_risk()` at `tools.rs:92` classifies installs by capabilities. `AgentInstallApprovalPolicy` (`Always`/`RiskBased`/`Never`) in config. Install flow has promotion gate + human approval gate + install ref validation (~100 lines at `tools.rs:2769-2866`). | No promotion gates. No install-specific approval policy. Generic `skill.store` and `approval.queue` primitives. Agents request approval themselves via `approval.queue.request()` before calling `agent.install`. | `autonoetic-gateway/src/runtime/tools.rs:84-122, 2769-2866`, `autonoetic-types/src/config.rs:110` |
| **Role-specific checks** | Hardcoded agent ID strings in gateway code: `requires_promotion_gate()` branches on `"specialized_builder.default"` / `"evolution-steward.default"`. `is_install_high_risk()` makes risk decisions. Gateway knows about role categories. | Gateway is agent-agnostic. Validates capabilities, not agent identities. `agent.discover` scores by declared capabilities (already implemented at `tools.rs`). Test fixtures may still use realistic agent IDs. | `autonoetic-gateway/src/runtime/tools.rs:84-122` |
| **Background state** | `ReevaluationState` already simplified to 5 fields (`retry_not_before`, `stale_goal_at`, `last_outcome`, `pending_scheduled_action`, `open_approval_request_ids`) at `background.rs:159-173`. However, `BackgroundState` has 15+ fields including `processed_inbox_event_ids`, `processed_task_keys`, `processed_approval_request_ids` at `background.rs:188-217`. | `ReevaluationState` ≤5 fields, all generic scheduling state. Remove `stale_goal_at`, `pending_scheduled_action` (domain-specific). Add `last_signal_name`, `last_signal_at`. `BackgroundState` fields reduced to generic scheduling only. State file conventions (`task.md`, `scratchpad.md`, `handoff.md`) documented as best practice, not enforced. | `autonoetic-types/src/background.rs:159-217`, `autonoetic-gateway/src/runtime/reevaluation_state.rs` |
| **Approval mechanism** | Approval is embedded in `agent.install` with `AgentInstallApprovalPolicy` config gate. `scheduler/approval.rs` (133 lines) handles pending/approved/rejected files but is scheduler-coupled. No standalone `approval.queue` tool exists. | Generic `approval.queue` primitive: `approval.queue.request(capability, evidence, reason?)`, `approval.queue.status(request_id)`, `approval.queue.list(filter?)`. Any agent can request approval for any purpose. Decoupled from `agent.install`. | `autonoetic-gateway/src/scheduler/approval.rs` (133 lines), `autonoetic-types/src/config.rs:110` |
| **Capability enum** | 8 variants: `SandboxFunctions`, `ReadAccess`, `WriteAccess`, `NetworkAccess`, `CodeExecution`, `AgentSpawn`, `AgentMessage`, `BackgroundReevaluation`. See capability.rs for current list. | Add `SchedulerSignal`, `SkillStore`, `ApprovalQueue`. Clean mapping to gateway primitives. | `autonoetic-types/src/capability.rs` |

### New primitives not in original plan (added during evolution)

These features were implemented after the simplification plan was written. They align with the "gateway provides primitives" philosophy and should be preserved:

| Feature | Description | File |
|---------|-------------|------|
| **Schema enforcement** | `DeterministicCoercionEnforcer` auto-coerces malformed `agent.spawn` payloads. Supports agent composition without structural precision. | `autonoetic-types/src/schema_enforcement.rs`, `autonoetic-gateway/src/runtime/tools.rs` (agent.spawn hook) |
| **Deterministic execution** | `execution_mode: script` agents bypass LLM lifecycle entirely. Gateway runs script directly in sandbox. Ultimate "dumb pipe" for procedural agents. | `autonoetic-types/src/agent.rs` (`ExecutionMode`), `autonoetic-gateway/src/execution.rs` |
| **Pre-install discovery** | `agent.exists` and `agent.discover` tools for reuse-first behavior. Complements simplification by reducing unnecessary installs. | `autonoetic-gateway/src/runtime/tools.rs` |
| **Agent-adapter.default** | Schema-driven wrapper generation via `middleware` (`pre_process`/`post_process`). Replaces overlay-based adaptation. | `autonoetic/agents/evolution/agent-adapter.default/` |
| **Session observability** | `trace session rebuild` / `trace session follow` with session index manifest. Gateway primitive for unified timeline reconstruction. | CLI `trace.rs`, gateway session index at `.gateway/sessions/<id>/index.json` |
| **Causal chain rotation** | Log segmentation by date/size with hash-chain continuity across segments. Retention/compression policy. | `autonoetic-gateway/src/causal_chain/rotation.rs` |

### New primitives inspired by external systems (Hermes-Agent comparison)

These features were identified by comparing Autonoetic with [Hermes-Agent](https://github.com/NousResearch/hermes-agent) (Nous Research). They fill gaps while staying aligned with the "gateway provides primitives" philosophy:

| Feature | Why It Matters | Suggested Primitive |
|---------|---------------|---------------------|
| **Session search (FTS5 + LLM summarization)** | Autonoetic has causal chains but no cross-session semantic search. Agents need to recall past conversations by intent, not just replay raw logs. | `session.search(query, agent_id?, limit?) → ranked sessions` |
| **Cron scheduling with NL support** | `scheduler.interval` exists but lacks natural-language scheduling ("every 20 minutes", "daily at 9am"). Hermes demonstrates the power of this pattern. | `scheduler.cron(expression_or_nl, agent_id, payload?)` |
| **Batch subagent spawning** | Agents spawn subagents one-by-one. For research/coding workloads, parallel fan-out is essential. | `agent.spawn_batch([spawn_specs]) → [results]` |
| **Code execution sandbox** | `CodeExecution` is low-level. A structured `sandbox.exec` with language, code, timeout, and result capture is more agent-friendly. | `sandbox.exec(language, code, timeout?, stdin?) → {stdout, stderr, exit_code}` |
| **Central tool registry** | Hermes' `tools/registry.py` pattern (schema collection, dispatch, availability checking, error wrapping) makes adding new primitives frictionless. Adopt for gateway tool management. | Internal refactoring of `tools.rs` to registry pattern |
| **Interrupt-and-redirect** | No mechanism to interrupt a running agent and redirect its goal mid-task. | `agent.interrupt(session_id, new_goal?)` |
| **RL learning hooks** | Hermes integrates with Atropos RL environments for trajectory-based training. Autonoetic should expose hooks for plugging future learning strategies without mandating any specific one. | `trajectory.record(step)`, `trajectory.export(session_id, format)` |
| **Skill progressive disclosure** | Hermes uses Anthropic's progressive disclosure pattern: metadata first (name + 1-line desc), full instructions on demand, linked files loaded lazily. Prevents context window bloat when agent has many skills. | Adopt for `skill.list` (metadata only) and `skill.view` (full content) |
| **Toolset composition** | Hermes' `toolsets.py` supports `includes` for composing toolsets from other toolsets (e.g., `debugging = terminal + web + file`). Enables reusable capability bundles. | Agent-level convention: agents declare `toolset` in SKILL.md, gateway resolves composition |
| **User profiling (Honcho)** | Hermes integrates Honcho for dialectic user modeling — the agent builds a persistent understanding of user preferences across sessions. Autonoetic's `knowledge.*` tools can support this pattern. | Convention: agents use `knowledge.store` with `scope: user_profile` for persistent user modeling |
| **Context compression** | Hermes auto-compresses conversation when approaching context limits. Autonoetic's `memory.summarize` covers this, but should support automatic triggering. | `memory.summarize` with auto-trigger in lifecycle (Phase 12) |
| **MCP skill integration** | Hermes exposes MCP servers as skills in its skills hub. Autonoetic should treat MCP tools as first-class skill citizens. | Convention: MCP-discovered tools registered as skills with metadata |

### Textual memory conventions (from `concepts.md` vision)

The original design vision in `docs/design/concepts.md` describes a text-native memory model where agents manage state through plain Markdown files that LLMs can natively read, edit, and reason about. The current implementation only partially realizes this:

- **Implemented now**: per-agent `state/` files, content-addressable storage, `knowledge.*` tools with SQLite backing
- **Not yet implemented**: automatic `state/summary.md` rollups, enforced `task.md`/`scratchpad.md`/`handoff.md` conventions, cross-session text-based recall for LLM context

This plan addresses the gap through Phase 13 (Textual Memory Conventions).

### Cognitive Capsule standardization (from `concepts.md` vision)

`docs/design/concepts.md` (line 41) describes Cognitive Capsules as portable agent bundles containing the full agent directory plus runtime closure. A basic `CapsuleManifest` struct exists in `autonoetic-types/src/capsule.rs`, but no CLI commands, import/export tooling, or standardized format exist yet. Phase 14 addresses this.

### Skill Loading (future feature)

**Note**: `skill.draft` was removed because it created files in `{agent_dir}/skills/` that were never loaded into the agent's context at wake time. The tool was a dead feature.

**Concept for future implementation**: "Skills" would be injectable context fragments that extend an agent's capabilities without spawning a new agent.

| Concept | Purpose | Lifecycle |
|---------|---------|-----------|
| **Agent** | Do something (spawn, execute, return) | Full session |
| **Skill** | Know something (inject into context) | At wake time |
| **Content** | Share something (read on demand) | On-demand |
| **Knowledge** | Remember something (durable facts) | Cross-session |

**Use cases for skills:**
- Style guides (Python best practices injected at wake)
- API references (internal endpoints documentation)
- Decision checklists (deployment criteria)
- Lessons learned (debugging patterns)

**If implemented, the flow would be:**
1. Author creates `{agent_dir}/skills/*.md` with knowledge content
2. At agent wake, gateway reads `skills/` directory and injects relevant content into system prompt
3. Agent gains new knowledge without LLM reasoning overhead

**Why not just use content.read?**
- Skills auto-inject at wake (no tool call needed)
- Skills can be selectively loaded based on task context
- Skills stay in context for the full session (vs one-time read)

---

## Phase 1: Kill Implicit Ingress Routing

**Problem**: Gateway resolves `event.ingest` without `target_agent_id` to `default_lead_agent_id`, persists session lead bindings, and has a `config.default_lead_agent_id` field. This embeds a delegation model into infrastructure.

**Goal**: Require explicit `target_agent_id` on all ingress. The routing intelligence belongs in agents, not in the gateway's request parsing.

### Tasks

**1.1 Remove implicit target resolution**

- **File**: `autonoetic-gateway/src/router.rs` (lines 460-486)
- **Action**: In `resolve_ingest_target_agent_id()`, remove the fallback chain. If `target_agent_id` is `None`, return a structured error: `{ error_type: "validation", message: "target_agent_id required for event.ingest", repair_hint: "Specify target_agent_id explicitly" }`. Remove the session lead binding read from disk.
- **Delete**: `session_lead_binding_path()` function (lines 541-570)
- **Delete**: `persist_session_lead_binding()` function (lines 572-616)

**1.2 Remove `default_lead_agent_id` from gateway config**

- **File**: `autonoetic-types/src/config.rs` (line ~30)
- **Action**: Remove `default_lead_agent_id` field from `GatewayConfig`
- **File**: Any config YAML files that set this field
- **Action**: Remove the field from example/test configs

**1.3 Update planner agent SKILL.md to handle routing**

- **File**: `autonoetic/agents/lead/planner.default/SKILL.md`
- **Action**: Add a section explaining that the planner is invoked by explicit `target_agent_id` or by terminal chat. The planner should instruct users/clients to target it explicitly. Add a `gateway.event.ingest` example showing how external callers should address the planner.
- **File**: Any other agents that relied on implicit routing
- **Action**: Update documentation to show explicit routing

**1.4 Update tests**

- **Files**: `autonoetic-gateway/src/router.rs` (test functions), `autonoetic-gateway/tests/`
- **Action**: Remove tests for implicit routing, session lead binding, default lead resolution. Add test that `event.ingest` without `target_agent_id` returns a structured validation error.

**1.5 Update CLI**

- **File**: `autonoetic/src/cli/chat.rs`
- **Action**: Terminal chat should accept a `--target-agent` flag, defaulting to `planner.default` for UX convenience. This is a CLI concern, not a gateway concern.
- **File**: `autonoetic/src/cli/agent.rs`
- **Action**: `agent run --interactive` should also accept `--target-agent` flag.

**1.6 Add `default_agent` to adapter configs (convention, not mandate)**

The planner becomes the default input agent **by convention**, not by mandate. The gateway has zero knowledge of what a "planner" is. Each adapter (CLI, WhatsApp, Discord, etc.) declares its own default target.

- **File**: New adapter config structure (documented in `protocols.md` or a new `adapters.md`)
- **Action**: Define a per-adapter config field:
  ```yaml
  # Example: WhatsApp adapter config
  adapter:
    type: whatsapp
    default_agent: planner.default   # convention: this adapter talks to planner
    # ...
  ```
  ```yaml
  # Example: Code review webhook adapter config
  adapter:
    type: webhook
    default_agent: auditor.default   # convention: this adapter talks to auditor
    # ...
  ```
- **File**: `autonoetic/src/cli/chat.rs`
- **Action**: CLI chat's `--target-agent` default value (`planner.default`) is the CLI's adapter convention.
- **Key distinction**: This is per-adapter configuration, not gateway routing logic. Each adapter is independently configurable. The gateway never resolves a missing target — the adapter provides it before the request reaches the gateway.

**Convention vs Mandate**:

| Aspect | Old (implicit routing) | New (adapter convention) |
|--------|----------------------|--------------------------|
| Where default lives | `GatewayConfig.default_lead_agent_id` | Per-adapter config |
| Gateway knowledge | Gateway resolves missing target | Gateway never sees missing target |
| Session bindings | Gateway persists lead bindings | No session bindings anywhere |
| Flexibility | One default for entire gateway | Different defaults per adapter |
| Nature | Platform-enforced | Deployment convention |

The planner is the **sensible default** for human-facing adapters (CLI chat, WhatsApp, Discord) because it understands ambiguous goals and delegates. But a webhook that always triggers code review targets `auditor.default` directly. A monitoring alert adapter might target `debugger.default`. The gateway doesn't care.

### Acceptance Criteria

- `event.ingest` without `target_agent_id` returns a structured error, never silently routes to a default
- `GatewayConfig` has no `default_lead_agent_id` field
- No session lead binding files are created or read
- Terminal chat CLI works with `--target-agent` flag, defaults to `planner.default`
- Each adapter can configure its own `default_agent`
- The planner is a convention (sensible default for human-facing adapters), not a mandate
- All routing decisions are made by the caller or adapter, never the gateway

---

## Phase 2: Externalize Wake Predicates

**Problem**: Gateway has 7 `WakeReason` variants with priority ordering in `should_wake()` (203 lines of decision logic). The gateway decides what "stale goal" means, what "retryable failure" means, etc. This is domain-specific heuristics.

**Goal**: Gateway provides `scheduler.interval(agent_id, period)` and `scheduler.signal(agent_id, name, payload)`. The agent decides what to do when woken.

### Tasks

**2.1 Simplify `WakeReason` to two variants**

- **File**: `autonoetic-types/src/background.rs` (lines 177-185)
- **Action**: Replace 7 variants (`Timer`, `NewMessage`, `TaskCompletion`, `QueuedWork`, `StaleGoal`, `RetryableFailure`, `ApprovalResolved`) with:
  ```rust
  pub enum WakeReason {
      Timer,           // periodic interval fired
      Signal { name: String, payload: Option<Value> },  // explicit signal from external source
  }
  ```

**2.2 Simplify `should_wake()`**

- **File**: `autonoetic-gateway/src/scheduler/decision.rs` (lines 26-122)
- **Action**: Replace the 203-line predicate chain with:
  1. Check if `next_due_at` has passed → `WakeReason::Timer`
  2. Check if inbox has unprocessed signals → `WakeReason::Signal { name, payload }` (consume one)
  3. Otherwise → `None`
- **Remove**: All `wake_predicates` checking (stale goal, retryable failure, task completions, queued work)
- **Remove**: `mark_reason_processed()` for individual predicate types
- **Keep**: Deduplication logic (`wake_fingerprint`, `active_session_ids`) — this is generic robustness

**2.3 Add `scheduler.signal()` gateway primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add a new native tool `scheduler.signal(agent_id, signal_name, payload?)` that writes a signal entry to the agent's inbox file. This is the generic mechanism for external systems (including other agents) to wake an agent with a named signal.
- **Capability**: Add `SchedulerSignal` to `Capability` enum in `autonoetic-types/src/capability.rs`
- **Policy**: In `policy.rs`, add `can_signal_agent()` check (does source agent have `SchedulerSignal` capability? is target agent ID allowed?)

**2.4 Simplify background mode**

- **File**: `autonoetic-types/src/background.rs`
- **Action**: Keep `BackgroundMode::Reasoning` (full LLM wake) and `BackgroundMode::Deterministic` (direct action execution). Remove the `pending_scheduled_action` complexity — agents decide what to do on tick via their SKILL.md instructions.

**2.5 Simplify `handle_due_wake()`**

- **File**: `autonoetic-gateway/src/scheduler/runner.rs` (lines 55-244)
- **Action**: Replace the three-path execution (approval resolved, pending action, reasoning) with:
  1. For `Timer` → fire full agent reasoning turn with a `tick` kickoff prompt
  2. For `Signal { name, payload }` → fire full agent reasoning turn with signal context in the kickoff prompt
- **Remove**: Direct execution of scheduled actions from the gateway (agents call `skill.execute` themselves in their reasoning turn)
- **Remove**: Approval-resolved direct action execution (agents check approval status themselves)

**2.6 Update agent SKILL.md files**

- **File**: `autonoetic/agents/lead/planner.default/SKILL.md` and all specialist SKILL.md files
- **Action**: Add reevaluation instructions. For example, in planner.default:
  ```
  On tick:
    1. Read state/reevaluation.json
    2. Check for pending approvals via approval.queue.status()
    3. Check for failed tasks that need retry
    4. If nothing to do, hibernate
  ```

**2.7 Update tests**

- **Files**: `autonoetic-gateway/src/scheduler/decision.rs`, `scheduler/runner.rs`, integration tests
- **Action**: Remove tests for individual wake predicates (stale goal, retryable failure, etc.). Add tests for timer and signal wakes. Add test for `scheduler.signal` tool.

### Acceptance Criteria

- `should_wake()` is ≤50 lines, checking only timer and inbox signals
- `WakeReason` has 2 variants, not 7
- `scheduler.signal` tool is available and policy-gated
- Gateway never decides what "stale" or "retryable" means — agents decide in SKILL.md
- Deduplication and backpressure still work (these are generic robustness, not domain logic)

---

## Phase 3: Simplify Disclosure Policy

**Problem**: 4 disclosure classes, 7+ source categories, path-pattern matching, taint tracking with length-sorted exact-substring replacement. Gateway must understand the *semantics* of data flow.

**Goal**: Simplify to tool-level `safe_to_repeat` tags. Tools declare whether their output can be repeated verbatim. The gateway applies a simple binary filter.

### Tasks

**3.1 Simplify `DisclosureClass` to binary**

- **File**: `autonoetic-types/src/disclosure.rs` (49 lines)
- **Action**: Replace 4-class system with:
  ```rust
  pub enum OutputClassification {
      Safe,      // can be repeated verbatim in assistant replies
      Restricted, // must not be repeated verbatim (secrets, raw credentials, etc.)
  }
  ```
- **Remove**: `DisclosureRule`, `DisclosurePolicy` with source/path/class matching
- **Replace with**: A simple `restricted_output_patterns: Vec<String>` in agent manifest for substring matching (gateway doesn't need to understand sources or paths)

**3.2 Simplify `DisclosureState`**

- **File**: `autonoetic-gateway/src/runtime/disclosure.rs` (210 lines)
- **Action**: Reduce to ~60 lines:
  - `register_restricted(value)` — mark a string as restricted
  - `filter_reply(reply)` — exact-substring replacement with `[REDACTED]`
  - Remove: `evaluate_class()`, source-based classification, path-based rules
  - Keep: taint tracking and reply filtering (this is generic safety, not domain logic)

**3.3 Remove origin-based classification from tool outputs**

- **File**: `autonoetic-gateway/src/runtime/tool_call_processor.rs`
- **Action**: Remove `register_result(source, path, result)` calls with origin categories. Replace with simple `register_restricted(result)` when a tool's metadata indicates `restricted: true`.
- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add `restricted_output: bool` field to `ToolMetadata`. Set `restricted_output: true` for tools that handle secrets or sensitive data (e.g., `secrets.get` if it existed). All other tools default to `restricted_output: false`.

**3.4 Update tool metadata**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (each tool's `extract_metadata()`)
- **Action**: Review each tool's metadata. Most tools (web.search, web.fetch, memory.read, sandbox.exec, etc.) produce `Safe` output. Only tools that could leak secrets should produce `Restricted` output.

**3.5 Update tests**

- **Files**: disclosure-related tests in `tool_call_processor.rs`, `lifecycle.rs`
- **Action**: Simplify tests to verify binary safe/restricted filtering. Remove tests for 4-class disclosure, source-based classification, path-pattern matching.

### Acceptance Criteria

- `DisclosureClass` enum has 2 variants, not 4
- Disclosure policy is ~60 lines, not 210
- No source categories (user_input, session_context, tier1_memory, etc.)
- Tools declare `restricted_output: bool`, gateway applies simple filter
- Secret redaction in logs remains (that's generic safety)

---

## Phase 4: Remove Evolution Orchestration from Gateway

**Problem**: Gateway has promotion gates for `specialized_builder.default` and `evolution-steward.default`, install approval flows, adaptation overlay composition — deeply specific to one autonoetic use case.

**Goal**: Gateway provides `skill.store` and `approval.queue` primitives. The evolution narrative is agent-authored.

### Tasks

**4.1 Remove promotion gate**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (lines ~86, ~4383-4413)
- **Action**: Remove `requires_promotion_gate()` function and all calls to it. Remove the special-case check for `"specialized_builder.default"` and `"evolution-steward.default"`.
- **File**: `autonoetic-gateway/src/runtime/tools.rs` (`agent.install` tool, lines 2410-2549+)
- **Action**: Remove the promotion gate step from `agent.install`. The install flow becomes: validate SKILL.md → write files → arm background schedule if requested. No role-specific gates.

**4.2 Remove `AgentInstallApprovalPolicy` gate**

- **File**: `autonoetic-types/src/config.rs`
- **Action**: Remove `AgentInstallApprovalPolicy` enum and its config field. Replace with a generic `approval.queue.request()` mechanism.
- **File**: `autonoetic-gateway/src/runtime/tools.rs` (`agent.install` tool)
- **Action**: Remove the `Always/RiskBased/Never` approval gate from install. If an agent wants approval before installing another agent, it calls `approval.queue.request()` itself in its reasoning loop before calling `agent.install`.

**4.3 Remove high-risk detection from gateway**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (`agent.install` tool)
- **Action**: Remove the logic that detects high-risk installs (CodeExecution, broad scopes, background-enabled). Agents that want to assess risk can do so in their SKILL.md instructions. The gateway still enforces capability boundaries at execution time.

**4.4 Add `skill.store` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add two new native tools:
  - `skill.store.publish(name, description, script, metadata?)` — stores a skill bundle in `agents/.gateway/skills/<name>/` with status `pending_approval`. Returns the skill ID.
  - `skill.store.approve(skill_id, approved, reason?)` — updates skill status to `approved` or `rejected`.
  - `skill.store.list()` — returns all approved skills with name + description
  - `skill.store.describe(name)` — returns full skill documentation (already exists as `skill.describe` or similar)
- **Capability**: Add `SkillStore` to `Capability` enum
- **Policy**: `can_publish_skill()`, `can_approve_skill()` checks

**4.5 Simplify `agent.adapt`**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (`agent.adapt` tool, lines 3242-3458)
- **Action**: Remove the promotion gate from adaptation. Simplify to: validate overlay → write adaptation record → done. No capability-addition gate (capability boundaries are enforced at execution time, not at adaptation time).

**4.6 Update agent SKILL.md files**

- **Files**: `autonoetic/agents/evolution/` — all three evolution agents
- **Action**: Rewrite as agent-authored workflows using gateway primitives. For example, `specialized_builder.default` becomes a regular agent that:
  1. Calls `skill.store.publish()` to create skills
  2. Calls `approval.queue.request()` to request approval
  3. Calls `agent.install()` to install agents
  4. No special gateway behavior needed

**4.7 Update tests**

- **Files**: Tests referencing promotion gates, install approval flows, evolution orchestration
- **Action**: Remove tests for gateway-level promotion gates. Add tests for `skill.store` tools. Add tests for generic `approval.queue` usage.

### Acceptance Criteria

- No role-specific logic in gateway code (no mentions of "specialized_builder", "evolution-steward", "promotion gate")
- `agent.install` has no approval gates — agents handle approval themselves via `approval.queue`
- `skill.store` tools exist and are policy-gated
- Evolution agents are regular agents with SKILL.md instructions, not special-cased by the gateway

---

## Phase 5: Remove Hardcoded Role/Specialist Knowledge

**Problem**: Gateway code references specific agent IDs (`planner.default`, `researcher.default`, `coder.default`, `auditor.default`, etc.) in tests, tool logic, and promotion gates. The gateway should be agnostic to what agents exist.

**Goal**: Gateway is agent-agnostic. It validates capabilities, not agent identities.

### Tasks

**5.1 Remove role-specific checks from tools.rs**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Search for all hardcoded agent ID strings and remove them. The only place agent IDs should appear is in:
  - Test fixtures (these are fine — they test with realistic data)
  - Capability checks (e.g., "does this agent have AgentSpawn capability?" — this is generic)
- **Remove**: Any logic that branches on specific agent IDs (e.g., "if agent_id == 'specialized_builder.default' then...")

**5.2 Remove `requires_promotion_gate()`**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (line ~86)
- **Action**: Already handled in Phase 4, but verify complete removal.

**5.3 Clean up `agent.discover` scoring**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (`agent.discover` tool, lines 2980-3241)
- **Action**: Ensure scoring is based on declared capabilities and descriptions, not hardcoded role names. If there's any special-casing of known roles, remove it.

**5.4 Update reference agent bundles**

- **Files**: `autonoetic/agents/lead/planner.default/SKILL.md`
- **Action**: The planner's Role Registry (v1) section is fine — it's agent-authored, not gateway logic. Verify it's in the SKILL.md body, not in gateway code.

**5.5 Update tests**

- **Action**: Keep tests that use realistic agent IDs (planner.default, researcher.default, etc.) as test fixtures — this is not the same as gateway code knowing about roles. Only remove tests that verify gateway *behavior changes* based on specific agent IDs.

### Acceptance Criteria

- No gateway code branches on specific agent IDs
- Gateway validates capabilities, not agent identities
- Test fixtures can still use realistic agent IDs
- `agent.discover` scores based on capabilities, not role names

---

## Phase 6: Simplify State Machine Conventions

**Problem**: Gateway prescribes specific state file conventions (task.md, scratchpad.md, handoff.md) and tracks complex `BackgroundState` with 15+ fields.

**Goal**: Gateway provides generic state persistence. Agents decide what conventions to use.

### Tasks

**6.1 Simplify `BackgroundState`**

- **File**: `autonoetic-types/src/background.rs` (`ReevaluationState`, lines ~200-258)
- **Action**: Remove fields that track domain-specific state:
  - Remove: `processed_inbox_hashes`, `processed_task_completion_ids`, `stale_goal_at`, `last_outcome`, `pending_scheduled_action`
  - Keep: `last_wake_at`, `next_due_at`, `retry_not_before` (generic scheduling state)
  - Add: `last_signal_name`, `last_signal_at` (for signal-based wakes)

**6.2 Remove `state/reevaluation.json` from gateway**

- **File**: `autonoetic-gateway/src/runtime/reevaluation_state.rs` (211 lines)
- **Action**: Simplify to ~80 lines:
  - `load_state(agent_id)` → reads minimal scheduling state
  - `persist_state(agent_id, state)` → writes minimal scheduling state
  - Remove: `execute_scheduled_action()` — agents call `skill.execute` themselves
  - Remove: all scheduled action execution logic

**6.3 Document conventions as best practice**

- **File**: `autonoetic/docs/` — new file or add to existing
- **Action**: Document `state/task.md`, `state/scratchpad.md`, `state/handoff.md` as recommended conventions for agent authors. Not enforced by the gateway.

### Acceptance Criteria

- `ReevaluationState` has ≤5 fields, all generic scheduling state
- Gateway doesn't prescribe state file conventions
- State conventions documented as agent best practices

---

## Phase 7: Add Generic `approval.queue` Primitive

**Problem**: Approval is currently embedded in `agent.install` with specific policy gates. We need a generic approval mechanism.

**Goal**: Gateway provides a generic approval queue that any agent can use for any purpose.

### Tasks

**7.1 Add approval queue tools**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tools:
  - `approval.queue.request(capability, evidence, reason?)` — creates an approval request, returns `request_id`
  - `approval.queue.status(request_id)` — returns pending/approved/rejected
  - `approval.queue.list(filter?)` — lists approval requests (for human-facing agents)
- **Capability**: `ApprovalQueue` in `autonoetic-types/src/capability.rs`
- **Policy**: `can_request_approval()`, `can_check_approval()` checks

**7.2 Reuse existing approval infrastructure**

- **File**: `autonoetic-gateway/src/scheduler/approval.rs` (133 lines)
- **Action**: Extend existing approval persistence to support the generic queue. The current approval system is already close — just needs to be decoupled from `agent.install` and made available as a standalone tool.

**7.3 Update CLI approval commands**

- **File**: `autonoetic/src/cli/gateway.rs`
- **Action**: Ensure `gateway approvals list|approve|reject` CLI commands work with the generic queue.

**7.4 Update tests**

- **Action**: Add tests for generic approval flow: agent requests approval, human approves via CLI, agent checks status and proceeds.

### Acceptance Criteria

- `approval.queue` tools exist and are policy-gated
- Approval queue is decoupled from `agent.install`
- CLI approval commands work with generic queue
- Any agent can request approval for any capability

---

## Phase 8: Clean Up Capability Enum

**Problem**: `Capability` enum has evolution-specific variants and may need updating after the above changes.

**Goal**: Clean, minimal capability set that maps to gateway primitives.

### Tasks

**8.1 Review and update `Capability` enum**

- **File**: `autonoetic-types/src/capability.rs`
- **Current**: `SandboxFunctions, ReadAccess, WriteAccess, NetworkAccess, CodeExecution, AgentSpawn, AgentMessage, BackgroundReevaluation`
- **Action**: Add new capabilities:
  - `SchedulerSignal` — can send signals to other agents
  - `SkillStore` — can publish/approve skills
  - `ApprovalQueue` — can request/check approvals
  - `SessionSearch` — can search across sessions
  - `AgentInterrupt` — can interrupt running agents
  - `TrajectoryExport` — can export execution trajectories for learning
  - `CapsuleExport` — can export/import Cognitive Capsules
- **Note**: `SandboxFunctions` replaces `ToolInvoke`. `ReadAccess` replaces `MemoryRead`/`MemorySearch`. `WriteAccess` replaces `MemoryWrite`/`MemoryShare`. `NetworkAccess` replaces `NetConnect`. `CodeExecution` replaces `ShellExec`.

**8.2 Update policy checks**

- **File**: `autonoetic-gateway/src/policy.rs` (267 lines)
- **Action**: Add policy check functions for new capabilities. Ensure all gateway primitives are gated by appropriate capabilities.

### Acceptance Criteria

- `Capability` enum is clean, minimal, and maps to gateway primitives
- All new tools have corresponding capability checks

---

## Phase 9: Update Agent SKILL.md Files

**Problem**: After removing gateway orchestration, agents need to express orchestration logic in their SKILL.md files.

**Goal**: All delegation, reevaluation, and evolution logic lives in agent-authored SKILL.md files.

### Tasks

**9.1 Rewrite planner.default SKILL.md**

- **File**: `autonoetic/agents/lead/planner.default/SKILL.md`
- **Action**:
  - Remove any references to gateway implicit routing
  - Add explicit delegation instructions using `agent.spawn`
  - Add reevaluation instructions using tick handling
  - Add evolution instructions using `skill.store` and `approval.queue`
  - Keep the Role Registry as agent-authored mapping (this is fine — it's in the agent, not the gateway)

**9.2 Rewrite specialist SKILL.md files**

- **Files**: `autonoetic/agents/specialists/*/SKILL.md`
- **Action**: Each specialist should declare its capabilities and describe how it interacts with other agents via gateway primitives. No changes to core behavior, just documentation updates.

**9.3 Rewrite evolution SKILL.md files**

- **Files**: `autonoetic/agents/evolution/*/SKILL.md`
- **Action**: Rewrite as regular agents using gateway primitives:
  - `specialized_builder.default`: Uses `skill.store.publish()`, `approval.queue.request()`, `agent.install()`
  - `evolution-steward.default`: Uses `approval.queue.list()`, `approval.queue.status()`, `skill.store.approve()`
  - `memory-curator.default`: Uses `memory.remember()`, `memory.recall()`

**9.4 Add example reevaluation section**

- **File**: New example in `autonoetic/examples/` or add to existing examples
- **Action**: Show a complete example of an agent with reevaluation logic in its SKILL.md, using gateway primitives.

### Acceptance Criteria

- All agent orchestration logic is in SKILL.md files
- No agent relies on gateway-specific behavior (implicit routing, promotion gates, etc.)
- Evolution agents are regular agents, not special-cased

---

## Phase 10: Session Search, Cron Scheduling & Batch Spawning

**Problem**: Autonoetic has causal chains for audit but no cross-session semantic search. The scheduler supports intervals but not natural-language cron expressions. Subagent spawning is serial-only.

**Goal**: Add three new gateway primitives that fill feature gaps identified in the Hermes-Agent comparison, aligned with the "dumb secure pipe" philosophy.

### Tasks

**11.1 Add `session.search` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `session.search(query, agent_id?, limit?)` that performs full-text search across session transcripts and causal chain entries. Returns ranked results with session_id, turn_id, and content snippet.
- **Implementation**: Use SQLite FTS5 extension on the existing `knowledge.db` or create a dedicated `sessions.db`. Index session transcripts on hibernate.
- **Capability**: `SessionSearch` in `autonoetic-types/src/capability.rs`
- **Policy**: Scope search to agent's own sessions by default; `SessionSearch` capability with `allowed_targets` for cross-agent search.

**11.2 Add `scheduler.cron` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `scheduler.cron(expression, agent_id, payload?)` that accepts either:
  - Standard cron expressions (`"0 9 * * *"` for daily at 9am)
  - Natural language (`"every 20 minutes"`, `"daily at 9am"`, `"weekly on Monday"`)
- **Implementation**: Parse NL to cron internally (simple pattern matching for common cases; LLM-based for complex ones). Store in gateway scheduler alongside interval-based schedules.
- **File**: `autonoetic-gateway/src/scheduler/cron.rs` (new)
- **Action**: Cron expression parser and NL-to-cron converter.

**11.3 Add `agent.spawn_batch` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `agent.spawn_batch([spawn_specs])` that spawns multiple subagents in parallel and returns results as a list. Each spec follows the same schema as `agent.spawn`.
- **Implementation**: Use Rust async to run spawns concurrently. Respect `max_concurrent_spawns` from config.
- **Capability**: Uses existing `AgentSpawn` capability (batch is just a convenience wrapper).

**11.4 Add `sandbox.exec` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `sandbox.exec(language, code, timeout?, stdin?)` that executes code in the sandbox with structured I/O:
  - `language`: `"python"`, `"javascript"`, `"bash"`, etc.
  - `code`: the source code string
  - `timeout`: optional execution timeout (default 30s)
  - `stdin`: optional stdin input
  - Returns: `{stdout, stderr, exit_code, duration_ms}`
- **Implementation**: Wraps existing sandbox drivers with language-specific execution templates. For Python: `python3 -c "$code"`. For JS: `node -e "$code"`.
- **Capability**: `SandboxExec` in `autonoetic-types/src/capability.rs`
- **Policy**: Subject to existing `CodeExecution` scoping patterns.

**11.5 Refactor tool registration to registry pattern**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (and new `tool_registry.rs`)
- **Action**: Extract tool registration into a declarative registry pattern (inspired by Hermes' `tools/registry.py`):
  ```rust
  pub struct ToolDefinition {
      pub name: &'static str,
      pub schema: Value,           // JSON Schema for parameters
      pub required_capability: Option<Capability>,
      pub restricted_output: bool,
      pub handler: Box<dyn ToolHandler>,
  }
  ```
- **Benefit**: Adding a new primitive becomes declaring a `ToolDefinition` + handler, not modifying scattered match statements.

**11.6 Add `agent.interrupt` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `agent.interrupt(session_id, new_goal?)` that gracefully stops a running agent session and optionally injects a new goal. The interrupted agent receives a signal with the reason.
- **Capability**: `AgentInterrupt` in `autonoetic-types/src/capability.rs`
- **Policy**: Only agents with `AgentInterrupt` targeting the specific agent (or parent agents interrupting children) are allowed.

**11.7 Update tests**

- **Action**: Add tests for each new primitive. Ensure batch spawning respects concurrency limits. Test cron NL parsing.

### Acceptance Criteria

- `session.search` returns ranked results across sessions
- `scheduler.cron` accepts both cron expressions and natural language
- `agent.spawn_batch` spawns multiple agents concurrently
- `sandbox.exec` executes code with structured I/O and timeout
- Tool registration follows declarative registry pattern
- `agent.interrupt` gracefully stops running sessions
- All new primitives have capability gates and policy checks

---

## Phase 11: Self-Learning Hooks

**Problem**: Hermes-Agent demonstrates a closed learning loop (skills from experience, self-improvement during use, RL training via trajectory compression). Autonoetic has evolution agents but no standardized hooks for plugging in learning strategies.

**Goal**: Add generic hooks for trajectory recording and learning strategy attachment, without mandating any specific learning approach. The gateway provides the hooks; agents (or external systems) decide how to learn.

### Tasks

**12.1 Add trajectory recording primitives**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tools:
  - `trajectory.record(step)` — records a learning step (state, action, reward, next_state) to the session's trajectory log. Lightweight; stored as JSONL alongside causal chain.
  - `trajectory.export(session_id, format?)` — exports the full trajectory for a session. Formats: `jsonl` (default), `csv`, `openai_chat` (for fine-tuning).
  - `trajectory.list(agent_id?, limit?)` — lists available trajectory files.
- **Storage**: `.gateway/trajectories/<session_id>.jsonl`
- **Capability**: `TrajectoryExport` in `autonoetic-types/src/capability.rs`

**12.2 Add learning hook registration (SKILL.md)**

- **File**: `autonoetic-types/src/agent.rs` (AgentManifest)
- **Action**: Add optional `learning` section to SKILL.md frontmatter:
  ```yaml
  learning:
    enabled: true
    strategy: "experience_replay"  # or "policy_gradient", "custom"
    reward_signal: "task_success"  # or "user_feedback", "custom"
    record_trajectory: true
    export_format: "jsonl"
  ```
- **Effect**: When `learning.enabled: true`, the gateway automatically records trajectory steps during reasoning loops and script execution.

**12.3 Add `learning.consolidate` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `learning.consolidate(agent_id, strategy?)` that triggers a consolidation pass:
  1. Gather recent trajectories for the agent
  2. Apply the configured learning strategy (experience replay, summarization, etc.)
  3. Update the agent's knowledge base or generate skill improvements
- **Note**: The gateway provides the hook; the actual learning logic is agent-authored or pluggable. Default implementation is a simple "summarize recent trajectories into `state/learnings.md`" pass.
- **Capability**: `TrajectoryExport` (uses existing capability)

**12.4 Add RL environment adapter interface**

- **File**: `autonoetic-gateway/src/learning/` (new module)
- **Action**: Define a trait for RL environment adapters:
  ```rust
  pub trait LearningEnvironment {
      fn record_step(&mut self, step: TrajectoryStep);
      fn export_trajectory(&self, format: ExportFormat) -> Result<Vec<u8>>;
      fn compute_reward(&self, step: &TrajectoryStep) -> f64;
  }
  ```
- **Implementation**: Start with a `SimpleEnvironment` that uses causal chain events as trajectory steps. Later adapters (Atropos, custom RL frameworks) implement the same trait.
- **Design principle**: Gateway provides the trait and storage; external systems provide the learning algorithms.

**12.5 Document learning patterns**

- **File**: `autonoetic/docs/learning.md` (new)
- **Action**: Document:
  - How to enable trajectory recording in SKILL.md
  - How to export trajectories for external RL training
  - How to implement custom learning strategies
  - Patterns for experience replay, policy improvement, skill distillation

**12.6 Update tests**

- **Action**: Add tests for trajectory recording during agent reasoning loops. Test export in multiple formats. Test `learning.consolidate` with mock strategy.

### Acceptance Criteria

- Trajectory recording works during both reasoning and script agent execution
- `trajectory.export` produces valid JSONL/CSV suitable for external RL training
- SKILL.md `learning` section enables/disables trajectory recording
- `learning.consolidate` hook exists (default implementation: summarize to state/learnings.md)
- RL environment adapter trait is defined and extensible
- No specific learning algorithm is mandated — the gateway provides hooks, agents/external systems provide strategies

---

## Phase 12: Textual Memory Conventions

**Problem**: `docs/design/concepts.md` describes a text-native memory model where agents manage state through plain Markdown files that LLMs can natively read, edit, and reason about. The current implementation has per-agent `state/` directories and `knowledge.*` tools, but lacks:
- Enforced conventions for `task.md`, `scratchpad.md`, `handoff.md`
- Automatic `state/summary.md` rollups when conversations grow long
- Cross-session text-based recall that feeds into LLM context (not just SQLite queries)

**Goal**: Implement the textual memory vision from `concepts.md` as agent best practices with gateway support primitives, not as enforced rules.

### Tasks

**13.1 Add `memory.summarize` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `memory.summarize(session_id, target_file?)` that:
  1. Takes the full conversation history for a session
  2. Uses the agent's LLM to generate a concise summary
  3. Writes the summary to `state/summary.md` (or specified target file)
  4. Returns the summary text
- **Use case**: Called by agents (or automatically by the gateway) when conversation history approaches context window limits.
- **Capability**: Uses existing `WriteAccess` capability

**13.2 Add automatic summary rollup hook**

- **File**: `autonoetic-gateway/src/runtime/lifecycle.rs`
- **Action**: Add optional auto-summarization at the end of reasoning loops:
  - If `auto_summarize: true` in agent manifest, and conversation exceeds `summarize_threshold` tokens
  - Gateway triggers `memory.summarize` before hibernation
  - Summary is written to `state/summary.md`
  - Next wake loads summary + recent turns instead of full history
- **Configuration** in SKILL.md:
  ```yaml
  memory:
    auto_summarize: true
    summarize_threshold: 8000  # tokens
    summary_file: "state/summary.md"
  ```

**13.3 Document textual state conventions**

- **File**: `autonoetic/docs/textual-memory.md` (new)
- **Action**: Document the recommended conventions from `concepts.md`:
  - `state/task.md`: authoritative checklist with status markers (`[ ]`, `[x]`, `[~]`, `[!]`)
  - `state/scratchpad.md`: short-lived working notes, cleared between major tasks
  - `state/handoff.md`: compact progress snapshot for delegation or resumption
  - `state/summary.md`: auto-generated conversation summaries for context management
  - `state/learnings.md`: distilled insights from `learning.consolidate`
- **Emphasis**: These are **conventions**, not enforced rules. Agents choose which files to use.

**13.4 Add `memory.recall_text` primitive**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tool `memory.recall_text(query, scope?, limit?)` that performs semantic search across an agent's textual memory files (`state/*.md`, `history/*.jsonl`) and returns relevant excerpts with file paths and line numbers.
- **Difference from `session.search`**: `session.search` searches across all sessions system-wide. `memory.recall_text` searches within an agent's own textual memory files, optimized for the LLM context window (returns compact, relevant excerpts).
- **Implementation**: FTS5 index on `state/` and `history/` directories. Re-index on file writes.
- **Capability**: Uses existing `ReadAccess` capability

**13.5 Add context file injection**

- **File**: `autonoetic-gateway/src/runtime/lifecycle.rs`
- **Action**: At agent wake, automatically inject content from `state/summary.md` (if exists) into the system prompt as "Previous session context". This gives the LLM immediate access to cross-session memory without explicit tool calls.
- **Configuration**: Controlled by `memory.inject_summary: true` in SKILL.md frontmatter.

**13.6 Update tests**

- **Action**: Add tests for auto-summarization threshold detection. Test `memory.recall_text` relevance. Test context injection at agent wake.

### Acceptance Criteria

- `memory.summarize` produces useful summaries written to `state/summary.md`
- Auto-summarization triggers when configured threshold is exceeded
- `memory.recall_text` returns relevant excerpts from agent's textual memory
- Context injection loads `state/summary.md` into system prompt at wake
- Textual memory conventions are documented as best practices (not enforced)
- LLMs can naturally read, edit, and reason about state files

---

## Phase 13: Cognitive Capsule Standardization

**Problem**: `docs/design/concepts.md` (line 41) describes Cognitive Capsules as portable agent exports containing the full agent bundle plus runtime closure. A basic `CapsuleManifest` struct exists in `autonoetic-types/src/capsule.rs`, but there are no CLI commands, import/export tooling, standardized format, or federation support.

**Goal**: Make Cognitive Capsules a first-class, standardized artifact format that enables agent portability across machines, gateways, and potentially across different agent platforms.

### Tasks

**14.1 Extend `CapsuleManifest` schema**

- **File**: `autonoetic-types/src/capsule.rs`
- **Action**: Extend the existing manifest with:
  ```rust
  pub struct CapsuleManifest {
      pub capsule_id: String,
      pub agent_id: String,
      pub mode: CapsuleMode,           // Thin | Hermetic
      pub created_at: String,
      pub entrypoint: String,
      pub runtime_lock: String,
      pub included_artifacts: Vec<IncludedArtifact>,
      pub gateway_runtime: Option<CapsuleGatewayRuntime>,
      pub redactions: Vec<String>,
      // New fields:
      pub version: String,             // Capsule format version
      pub skills: Vec<String>,         // List of included skill names
      pub memory_summary: Option<String>,  // Optional memory snapshot
      pub causal_chain_hash: Option<String>,  // Integrity hash of included chain
      pub signature: Option<CapsuleSignature>,  // Cryptographic signature
  }

  pub struct CapsuleSignature {
      pub algorithm: String,           // "ed25519", "rsa-pss"
      pub public_key: String,
      pub signature: String,
  }
  ```

**14.2 Add `capsule.export` CLI command**

- **File**: `autonoetic/src/cli/capsule.rs` (new)
- **Action**: Add CLI command `autonoetic capsule export <agent_id> [options]`:
  - `--mode thin|hermetic` (default: thin)
  - `--include-artifacts` — bundle content-addressed artifacts
  - `--include-trajectories` — include trajectory logs
  - `--include-causal-chain` — include full causal chain
  - `--sign <key_path>` — sign the capsule with a private key
  - `--output <path>` — output path (default: `<agent_id>.capsule.tar.gz`)
- **Implementation**: Packages the agent directory + capsule.json manifest into a tar.gz archive.

**14.3 Add `capsule.import` CLI command**

- **File**: `autonoetic/src/cli/capsule.rs`
- **Action**: Add CLI command `autonoetic capsule import <capsule_path> [options]`:
  - `--target-dir <path>` — where to unpack (default: `agents/`)
  - `--verify-signature` — verify capsule signature before import
  - `--dry-run` — validate without importing
- **Implementation**: Unpacks tar.gz, validates manifest, verifies signature if present, installs agent.

**14.4 Add `capsule.verify` CLI command**

- **File**: `autonoetic/src/cli/capsule.rs`
- **Action**: Add CLI command `autonoetic capsule verify <capsule_path>` that:
  1. Validates manifest schema
  2. Verifies SHA-256 hashes of all included artifacts
  3. Checks signature if present
  4. Validates runtime.lock compatibility
  5. Reports any issues

**14.5 Define capsule format specification**

- **File**: `autonoetic/docs/capsule-format.md` (new)
- **Action**: Document the standardized capsule format:
  - Archive structure (`capsule.json` at root, agent files, optional artifacts/)
  - Manifest schema (all fields, types, required/optional)
  - Mode differences (thin vs hermetic)
  - Signature format and verification
  - Version compatibility rules
  - Cross-platform considerations

**14.6 Add capsule primitives to gateway**

- **File**: `autonoetic-gateway/src/runtime/tools.rs`
- **Action**: Add native tools:
  - `capsule.export(agent_id, mode?, options?) → capsule_path` — export from running gateway
  - `capsule.import(capsule_path, options?) → agent_id` — import into running gateway
  - `capsule.verify(capsule_path) → verification_result`
- **Capability**: `CapsuleExport` in `autonoetic-types/src/capability.rs`

**14.7 Design for cross-platform portability**

- **File**: `autonoetic/docs/capsule-format.md`
- **Action**: Design the capsule format to be spec-compatible with potential future non-Autonoetic runtimes:
  - `capsule.json` uses standard JSON Schema
  - `runtime.lock` format is language-agnostic (not Rust-specific)
  - Agent instructions are pure Markdown
  - Skills are pure text (Markdown + script files)
  - No Autonoetic-specific binary formats
- **Goal**: A capsule exported from Autonoetic should be theoretically importable by any compatible agent runtime.

**14.8 Update tests**

- **Action**: Add tests for export/import round-trip. Test signature creation and verification. Test thin vs hermetic modes. Test manifest validation.

### Acceptance Criteria

- `CapsuleManifest` schema is extended with version, skills, memory summary, signature
- `autonoetic capsule export|import|verify` CLI commands work
- Capsule format is documented as a portable specification
- Capsules can be signed and verified
- Gateway tools exist for programmatic capsule operations
- Format is designed for cross-platform portability (not Autonoetic-locked)

---

## Phase 14: Tool Registry, Skill Repository & Progressive Disclosure

**Problem**: Tools are hardcoded in `tools.rs` via scattered match statements. Adding a tool means modifying gateway code in multiple places. There's no skill system separate from agents — despite `concepts.md` describing "injectable context fragments." All agent context loads at once, causing context window bloat.

**Goal**: A declarative tool registry (Hermes-inspired, ~150 lines), a skill repository with progressive disclosure (3 tiers), and MCP tools as first-class citizens. The gateway provides the infrastructure; agents decide what to load.

### Key Insight: Skills ≠ Agents ≠ Tools

| Entity | What It Is | Where It Lives | Lifecycle |
|--------|-----------|---------------|-----------|
| **Tool** | Executable gateway primitive | Gateway code (Rust) | Registered at startup |
| **Agent** | SKILL.md + runtime + state | `{data}/agents/{id}/` | Full session |
| **Skill** | Injectable context fragment | `{data}/skills/store/{hash}/` (shared, content-addressable) | At wake time |

- **Tools** are executable. The gateway runs them. They're Rust code.
- **Skills** are contextual. The gateway injects them. They're Markdown.
- **Agents** are full runtimes. The gateway manages them. They're directories.
- **Agent composition** uses `agent.spawn` delegation — no manifest-level inheritance needed.

### Design Reference

See `autonoetic/docs/tool-skill-repository-design.md` for the full design document.

### Tasks

**14.1 Create Tool Registry (~150 lines)**

- **File**: `autonoetic-gateway/src/runtime/tool_registry.rs` (new)
- **Action**: Create a declarative tool registry replacing scattered match statements:
  ```rust
  pub struct ToolEntry {
      pub name: String,              // "content.write"
      pub schema: Value,             // OpenAI-format JSON Schema
      pub handler: ToolHandler,      // fn(args, ctx) -> Result<Value>
      pub capability: Capability,    // Required capability
      pub check_fn: Option<CheckFn>, // Availability check
      pub description: String,
  }

  pub struct ToolRegistry {
      tools: HashMap<String, ToolEntry>,
  }
  ```
  - `register(entry)` — add a tool
  - `get_definitions(names)` — return OpenAI-format schemas for LLM
  - `dispatch(name, args, ctx)` — execute with error wrapping
  - `available_tools(capabilities)` — filter by agent's declared capabilities
- **Migration**: Each tool in `tools.rs` becomes a `register_tool!` macro call in its own module. The giant match statement becomes `registry.dispatch()`.
- **Benefit**: Adding a tool = one file + one macro call. Schema co-located with handler.

**14.2 Create Skill Repository (~200 lines)**

- **File**: `autonoetic-gateway/src/skills/repository.rs` (new)
- **Store**: `{gateway_data}/skills/store/{sha256}/` — content-addressable, deduplicated
- **Registry**: `{gateway_data}/skills/registry.json` — name → hash mapping
- **Catalog**: `{gateway_data}/skills/catalog.json` — searchable index (name, desc, tags, source)
- **SKILL.md format** (YAML frontmatter):
  ```yaml
  ---
  name: nitter-scraper
  description: Scrape Twitter/X via Nitter instances
  version: 1.0.0
  tags: [web, scraping, twitter]
  platforms: [linux, macos]
  required_env: [NITTER_INSTANCE]
  ---
  # Full instructions here
  ```
- **Constraints**: name ≤64 chars, description ≤1024 chars
- **Content-addressable**: Same content → same hash → stored once. Skills shared by multiple agents are never duplicated.
- **Capsule export**: Bundle resolved skill content into capsule.zip (self-contained). On import, deduplicate by hash.

**14.3 Add `skill.list` and `skill.view` tools (~100 lines)**

- **File**: `autonoetic-gateway/src/runtime/tools.rs` (add skill.* tools)
- **`skill.list`** — local skills only (paginated, no "dump all"):
  - `skill.list(query?, category?, offset?, limit?) → {skills, total, has_more}`
  - Searches the local shared store only (catalog.json)
  - Default limit=20, requires query or category filter
- **`skill.view`** — full content on demand:
  - `skill.view(name, file?) → content`
  - Loads full SKILL.md or a specific linked file (references/, templates/, scripts/)
- **`skill.categories`** — category overview:
  - `skill.categories() → [{name, description, skill_count}]`
- **Wake sequence** (auto-injected):
  ```
  1. Load SKILL.md (agent instructions + skills list from frontmatter)
  2. Gateway resolves skills: registry.json → store/{hash}/SKILL.md
  3. Each skill's SKILL.md injected into context
  4. Agent can access linked files: skill.view("nitter-scraper", "references/api.md")
  ```
- **Agent declares skills in SKILL.md frontmatter** (no separate manifest file):
  ```yaml
  ---
  name: coder.default
  metadata:
    autonoetic:
      skills: [python-patterns, rust-idioms, fastapi-builder]
  ---
  ```
- **Capability**: New `SkillStore` capability (from Phase 8)

**14.4 Add Skill Hub for multi-source discovery (~250 lines)**

- **File**: `autonoetic-gateway/src/skills/hub.rs` (new)
- **Sources**:
  - `Local` — `~/.autonoetic/skills/`
  - `GitHub { repo, path }` — via GitHub Contents API
  - `WellKnown { url }` — `/.well-known/skills/index.json` endpoint
  - `Mcp { server }` — MCP server exposes skills
- **Trust levels**: `local` (trusted), `github:openai/` (trusted), `github:user/` (community)
- **Hub tools** (multi-source, distinct from local `skill.list`):
  - `skill.search(query, sources?, limit?) → [results]` — search across remote sources (GitHub, well-known, MCP). Returns metadata only. Agent reviews results, then calls `skill.install`.
  - `skill.install(source, name?) → result` — install from any source into local store
  - `skill.uninstall(name) → result` — remove a skill from local store
- **Difference from `skill.list`**: `skill.list` searches local store (already installed). `skill.search` discovers new skills from remote sources (not yet installed).
- **Hub metadata**: `{gateway_data}/skills/.hub/lock.json` tracks provenance
- **Security**: Skills scanned before installation (reuse existing skill guard pattern from Hermes)

**14.5 Register MCP tools as first-class registry entries**

- **File**: `autonoetic-gateway/src/skills/mcp_adapter.rs` (new, ~100 lines)
- **Action**: MCP tools discovered at connection time register into the tool registry:
  ```rust
  registry.register(ToolEntry {
      name: "mcp.github.list_issues",
      schema: mcp_tool_schema,
      handler: mcp_handler("github", "list_issues"),
      capability: Capability::ToolInvoke,
      check_fn: Some(|| mcp_server_connected("github")),
      description: "List GitHub issues via MCP",
  });
  ```
- **Appear in** `skill.list` with `source: "mcp"` tag
- **No separate MCP system** — MCP tools are just tools

**14.6 Add toolset convention to SKILL.md**

- **File**: `autonoetic/docs/AGENTS.md` (update)
- **Action**: Document toolset as an agent convention:
  ```yaml
  capabilities:
    - type: ToolInvoke
      allowed: ["content.", "knowledge.", "web.", "sandbox."]
      toolset: research  # Convention name, not enforced by gateway
  ```
- The gateway checks `allowed` prefixes (capability-based enforcement)
- The `toolset` name is informational — for humans and discovery
- **No gateway-level toolset code** — it's a convention, not a mandate

**14.7 Add `requires` field for simple dependency declaration**

- **File**: `autonoetic-gateway/src/runtime/lifecycle.rs` (wake validation)
- **Action**: Support a `requires` field in agent SKILL.md frontmatter:
  ```yaml
  ---
  name: fullstack-coder.default
  requires:
    agents: [architect.default, coder.default]
    skills: [python-patterns, fastapi-builder]
  ---
  ```
- **At wake**: Gateway checks if required agents/skills exist. If not, injects warning into agent context:
  ```
  Warning: architect.default not installed. agent.spawn('architect.default') will fail.
  ```
- **At capsule export**: `requires.agents` listed in capsule manifest. Importing gateway warns user about missing agent dependencies.
- **No dependency resolution**: No fetching, no version matching, no transitive deps. Just a declaration the gateway can read. Agent adapts its plan based on the warning.
- **Capability**: No new capability needed — gateway does this automatically at wake

**14.8 Update agent SKILL.md files with skill loading instructions**

- **Files**: All `autonoetic/agents/*/SKILL.md`
- **Action**: Add skill loading instructions and `requires` field to each agent:
  ```yaml
  ---
  name: coder.default
  requires:
    skills: [python-patterns, rust-idioms]
  ---
  On wake:
    1. Check skill availability warnings
    2. Call skill.list() to browse available skills
    3. If task requires a skill, call skill.view("skill-name")
    4. Use skill instructions to complete the task
  ```

**14.9 Update tests**

- **Files**: `autonoetic-gateway/src/skills/` tests, `tool_registry.rs` tests
- **Action**: Test registry registration, schema collection, dispatch. Test skill discovery, progressive disclosure, multi-source installation. Test `requires` field validation at wake.

### Acceptance Criteria

- Adding a tool requires one file + one macro call (no match statement editing)
- `skill.list` is paginated (default limit=20), always requires query or category filter
- `skill.view` loads full content on demand
- Skills stored in content-addressable shared store (deduplicated by hash)
- Agents declare skills in SKILL.md frontmatter `skills:` field (no separate manifest)
- Capsule export bundles skill content (self-contained), import deduplicates by hash
- MCP tools register as first-class registry entries
- Agent wake resolves skills from frontmatter → registry → store
- Toolsets are a convention in SKILL.md, not gateway code
- Agent composition uses `agent.spawn` delegation, not manifest inheritance
- Total new code: ~850 lines

---

## Phase 15: Documentation & Validation

### Tasks

**15.1 Update architecture docs**

- **Files**: `autonoetic/docs/architecture-summary.md`, `autonoetic/docs/separation-of-powers.md`
- **Action**: Update to reflect the simplified gateway. Ensure consistency with the implemented primitives.

**15.2 Update `concepts.md`**

- **File**: `autonoetic/concepts.md`
- **Action**: Update sections that reference removed gateway behaviors. Ensure the "Design Philosophy & Mandatory Rules" section reflects the new simplicity.

**15.3 Update `plan.md`**

- **File**: `autonoetic/plan.md`
- **Action**: Add this plan as Phase 10 (or appropriate phase number). Mark completed tasks.

**15.4 End-to-end integration test**

- **File**: New test in `autonoetic-gateway/tests/`
- **Action**: Add a test that proves the simplified architecture works:
  1. Terminal chat targets `planner.default` explicitly
  2. Planner delegates to `researcher.default` via `agent.spawn`
  3. Researcher stores findings via `memory.remember`
  4. Planner checks background tick and reevaluates state
  5. Planner requests approval via `approval.queue.request()`
  6. Approval granted via CLI
  7. Planner proceeds with approved action
  8. All events logged to causal chain

**15.5 Run full test suite**

- **Action**: `cargo test --workspace` — all tests pass
- **Action**: `cargo clippy --workspace` — no warnings

### Acceptance Criteria

- Architecture docs match implemented code
- Full test suite passes
- No clippy warnings
- End-to-end test proves the simplified architecture works

---

## Summary of Changes

| Area | Before | After |
|------|--------|-------|
| **Ingress routing** | 3-step fallback chain, session lead bindings, `default_lead_agent_id` config | Explicit `target_agent_id` required; per-adapter `default_agent` convention, not gateway logic |
| **Wake predicates** | 7 `WakeReason` variants, 203-line predicate chain, direct scheduled action execution | 2 variants (`Timer`, `Signal`), ~40-line decision, agent decides what to do |
| **Disclosure** | 4 classes, 7+ source categories, path-pattern matching, 210 lines | Binary safe/restricted, tool-level tag, ~60 lines |
| **Evolution** | Promotion gates, install approval policy, role-specific checks, adaptation gates | Generic `skill.store` and `approval.queue` primitives |
| **Roles** | Hardcoded agent IDs in gateway code | Agent-agnostic, capability-based validation |
| **State machine** | 15+ field `ReevaluationState`, prescribed file conventions | ≤5 field generic state, conventions documented as best practice |
| **Approval** | Embedded in `agent.install` with role-specific gates | Generic `approval.queue` primitive, any agent can use |
| **Session search** | Causal chain replay only (CLI) | `session.search` with FTS5 across all sessions |
| **Scheduling** | Interval-based only | `scheduler.cron` with cron expressions + natural language |
| **Subagent spawning** | Serial, one-by-one | `agent.spawn_batch` for parallel fan-out |
| **Code execution** | Low-level `CodeExecution` | Structured `sandbox.exec` with language, timeout, result capture |
| **Tool registration** | Scattered match statements | Declarative registry pattern (schema + handler + capability) |
| **Agent interruption** | Not possible | `agent.interrupt` for graceful stop + goal redirect |
| **Self-learning** | No hooks | Trajectory recording, export, learning consolidation hooks |
| **Textual memory** | Convention only in docs | `memory.summarize`, auto-rollup, `memory.recall_text`, context injection |
| **Cognitive Capsule** | Basic struct, no tooling | Full export/import/verify CLI, signed format, cross-platform spec |
| **Tool/skill repository** | Scattered match statements, no skill system | Declarative tool registry + skill repository with progressive disclosure (3 tiers) |
| **Skill system** | No skills separate from agents | Shared skill repository with progressive disclosure, multi-source hub (local/GitHub/well-known/MCP) |
| **Toolset composition** | Flat tool lists | Convention in SKILL.md (not gateway code) |
| **User profiling** | No cross-session user modeling | `knowledge.store` with `scope: user_profile` convention |
| **MCP as tools** | MCP tools separate from gateway tools | MCP tools register as first-class entries in the same tool registry |
| **Capability enum** | 10 variants | 18 variants (8 new: SessionSearch, SandboxExec, AgentInterrupt, TrajectoryExport, CapsuleExport, SchedulerSignal, SkillStore, ApprovalQueue)

## Files Modified (Estimated)

### Phases 1-10 (Gateway Simplification)

| File | Change Type |
|------|-------------|
| `autonoetic-types/src/background.rs` | Major rewrite (~150 lines removed) |
| `autonoetic-types/src/disclosure.rs` | Major rewrite (~30 lines removed) |
| `autonoetic-types/src/config.rs` | Remove 2 fields |
| `autonoetic-types/src/capability.rs` | Add 8 variants, rename 1 |
| `autonoetic-gateway/src/router.rs` | Remove ~200 lines (implicit routing, lead bindings) |
| `autonoetic-gateway/src/scheduler/decision.rs` | Major rewrite (~160 lines removed) |
| `autonoetic-gateway/src/scheduler/runner.rs` | Major rewrite (~150 lines removed) |
| `autonoetic-gateway/src/runtime/tools.rs` | Add ~300 lines (new tools), remove ~200 lines (gates, role checks) |
| `autonoetic-gateway/src/runtime/disclosure.rs` | Major rewrite (~150 lines removed) |
| `autonoetic-gateway/src/runtime/reevaluation_state.rs` | Major rewrite (~130 lines removed) |
| `autonoetic-gateway/src/policy.rs` | Add ~50 lines (new capability checks) |
| `autonoetic/agents/*/SKILL.md` | Rewrite (9 agent bundles) |
| `autonoetic/src/cli/chat.rs` | Add `--target-agent` flag |
| `autonoetic/docs/` | Update architecture docs |

### Phases 11-14 (New Primitives)

| File | Change Type |
|------|-------------|
| `autonoetic-gateway/src/runtime/tools.rs` | Add ~800 lines (session.search, scheduler.cron, sandbox.exec, agent.spawn_batch, agent.interrupt, memory.summarize, memory.recall_text, trajectory.*, learning.*, capsule.*) |
| `autonoetic-gateway/src/runtime/tool_registry.rs` | New file (~150 lines) — declarative tool registration |
| `autonoetic-gateway/src/scheduler/cron.rs` | New file (~200 lines) — cron expression parser + NL converter |
| `autonoetic-gateway/src/learning/mod.rs` | New file (~100 lines) — LearningEnvironment trait |
| `autonoetic-gateway/src/runtime/lifecycle.rs` | Add auto-summarization hook (~80 lines) |
| `autonoetic-gateway/src/runtime/content_store.rs` | Add FTS5 indexing for session search (~100 lines) |
| `autonoetic-types/src/agent.rs` | Add `learning` and `memory` sections to AgentManifest (~60 lines) |
| `autonoetic-types/src/capsule.rs` | Extend CapsuleManifest with signature, version, skills (~80 lines) |
| `autonoetic/src/cli/capsule.rs` | New file (~300 lines) — capsule export/import/verify CLI |
| `autonoetic/src/cli/mod.rs` | Add `capsule` subcommand |
| `autonoetic/docs/learning.md` | New file — learning hooks documentation |
| `autonoetic/docs/textual-memory.md` | New file — textual memory conventions |
| `autonoetic/docs/capsule-format.md` | New file — Cognitive Capsule format specification |

### Phase 14 (Tool Registry & Skill Repository)

| File | Change Type |
|------|-------------|
| `autonoetic-gateway/src/runtime/tool_registry.rs` | New file (~150 lines) — declarative tool registration, schema collection, dispatch |
| `autonoetic-gateway/src/skills/repository.rs` | New file (~200 lines) — filesystem skill repository, SKILL.md parsing |
| `autonoetic-gateway/src/skills/hub.rs` | New file (~250 lines) — multi-source skill discovery (local, GitHub, well-known, MCP) |
| `autonoetic-gateway/src/skills/mcp_adapter.rs` | New file (~100 lines) — MCP tools register as first-class registry entries |
| `autonoetic-gateway/src/skills/mod.rs` | New file (~50 lines) — skill module exports |
| `autonoetic-gateway/src/runtime/tools.rs` | Add ~200 lines (skill.list, skill.view, skill.install, skill.uninstall, skill.search, skill.categories) |
| `autonoetic/agents/*/SKILL.md` | Update with skill loading instructions (9 agent bundles) |
| `autonoetic/docs/tool-skill-repository-design.md` | New file — full design document |
| `autonoetic/docs/AGENTS.md` | Update with toolset convention documentation |

**Net effect (all phases)**: ~800 lines of domain-specific gateway logic removed, ~3,850 lines of generic primitives added. Gateway remains simple; agents gain significant new capabilities through composition.

## Migration Notes

Phases 10-15 are **additive** — they don't break existing behavior. New primitives can be added incrementally:

1. **Phase 10** (session search, cron, batch) can ship in any order — each tool is independent
2. **Phase 11** (learning hooks) depends on trajectory storage from Phase 10 but is otherwise standalone
3. **Phase 12** (textual memory) builds on existing `state/` conventions and `memory.summarize` from Phase 10
4. **Phase 13** (capsules) extends existing `capsule.rs` and is independent of other phases
5. **Phase 14** (tool registry + skill repository) can be incremental: registry first (no behavior change), then skills
6. **Phase 15** (documentation) is pure documentation — no code changes

Recommended sequencing:
- **Phase 10** first (fills immediate gaps: search, cron, batch, sandbox)
- **Phase 14** second (tool registry + skill repository — refactors core infrastructure)
- **Phase 12** third (textual memory — highest agent impact for daily use)
- **Phase 13** fourth (capsules — enables portability and sharing)
- **Phase 11** fifth (learning hooks — most experimental, benefits from all prior phases)
- **Phase 15** last (document everything)

### Design Heritage Note

Phases 10-14 draw from multiple sources but remain original to Autonoetic:
- **Hermes-Agent registry pattern** (229 lines Python) → Autonoetic `tool_registry.rs` (~150 lines Rust) — tools self-register with schema + handler
- **Hermes-Agent skill progressive disclosure** (3 tiers) → Autonoetic skill repository — metadata first, full content on demand
- **Hermes-Agent skills hub** (multi-source) → Autonoetic `skills/hub.rs` — local + GitHub + well-known + MCP sources
- **CCOS patterns** (learning, consolidation) → Agent-driven via `skill.store` + `trajectory.export`
- **concepts.md vision** (Global Skill Engine Repository) → Shared `~/.autonoetic/skills/` directory

The goal is proven patterns, not copied complexity. Autonoetic's design is original.
