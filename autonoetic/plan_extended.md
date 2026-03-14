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
| **Capability enum** | 10 variants: `ToolInvoke`, `MemoryRead`, `MemoryWrite`, `MemoryShare`, `MemorySearch`, `NetConnect`, `AgentSpawn`, `AgentMessage`, `BackgroundReevaluation`, `ShellExec`. No scheduler signal or skill store capabilities. | Add `SchedulerSignal`, `SkillStore`, `ApprovalQueue`. Rename `BackgroundReevaluation` → `SchedulerInterval`. Clean mapping to gateway primitives. | `autonoetic-types/src/capability.rs` (40 lines) |

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
- **Action**: Remove the logic that detects high-risk installs (ShellExec, broad scopes, background-enabled). Agents that want to assess risk can do so in their SKILL.md instructions. The gateway still enforces capability boundaries at execution time.

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

- **File**: `autonoetic-types/src/capability.rs` (40 lines)
- **Current**: `ToolInvoke, MemoryRead, MemoryWrite, MemoryShare, MemorySearch, NetConnect, AgentSpawn, AgentMessage, BackgroundReevaluation, ShellExec`
- **Action**: Add new capabilities:
  - `SchedulerSignal` — can send signals to other agents
  - `SkillStore` — can publish/approve skills
  - `ApprovalQueue` — can request/check approvals
- **Remove**: `BackgroundReevaluation` — rename to `SchedulerInterval` (more generic)

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

## Phase 10: Documentation & Validation

### Tasks

**10.1 Update architecture docs**

- **Files**: `autonoetic/docs/architecture-summary.md`, `autonoetic/docs/separation-of-powers.md`
- **Action**: Update to reflect the simplified gateway. Ensure consistency with the implemented primitives.

**10.2 Update `concepts.md`**

- **File**: `autonoetic/concepts.md`
- **Action**: Update sections that reference removed gateway behaviors. Ensure the "Design Philosophy & Mandatory Rules" section reflects the new simplicity.

**10.3 Update `plan.md`**

- **File**: `autonoetic/plan.md`
- **Action**: Add this plan as Phase 10 (or appropriate phase number). Mark completed tasks.

**10.4 End-to-end integration test**

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

**10.5 Run full test suite**

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

## Files Modified (Estimated)

| File | Change Type |
|------|-------------|
| `autonoetic-types/src/background.rs` | Major rewrite (~150 lines removed) |
| `autonoetic-types/src/disclosure.rs` | Major rewrite (~30 lines removed) |
| `autonoetic-types/src/config.rs` | Remove 2 fields |
| `autonoetic-types/src/capability.rs` | Add 3 variants, rename 1 |
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

**Net effect**: ~800 lines of domain-specific gateway logic removed, ~350 lines of generic primitives added. Gateway becomes simpler, agents become more capable.
