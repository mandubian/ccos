# Workflow-Oriented Orchestration Plan

This plan replaces the current brittle "resume sessions and hope the right agent wakes" model with a durable workflow model owned by the gateway.

It builds on the findings in:

- `autonoetic/plan_signal.md`
- `autonoetic/docs/approval-notification-delivery.md`
- the current trace/timeline infrastructure in `autonoetic-gateway/src/runtime/session_timeline.rs`
- the current CLI trace surface in `autonoetic/src/cli/trace.rs`

## Problem

Today the system behaves as if the durable unit is a process or session:

- the planner runs in one session
- child agents run in child sessions
- approvals target child sessions
- chat watches only one session at a time
- planner continuation depends on signal routing and process lifetime details

This creates a fragile control flow:

- planner turns end too eagerly
- child approvals resume only the child runtime
- parent planner does not reliably continue
- parent CLI chat often sees no continuation
- session-id parsing is used where explicit orchestration metadata should exist

The core mistake is architectural:

- the real durable thing is not the planner process
- the real durable thing is the workflow run

## Goals

- Make approval a first-class workflow barrier instead of a session/signal hack.
- Keep planner intent alive across child execution, approval pauses, and restarts.
- Allow planners to spawn multiple child agents in parallel.
- Allow multiple blocked children to resume in parallel after approval.
- Make child completion, approval blocking, planner resume, and final synthesis observable in real time.
- Expose the live workflow state in the CLI, not only in `timeline.md`.

## Non-Goals

- Do not make the gateway semantically understand every agent role.
- Do not hardcode "planner/coder/evaluator" business logic into the gateway.
- Do not keep long-lived agent processes alive just to preserve orchestration state.
- Do not use session path parsing as the main orchestration mechanism.

## Core Model

Introduce a durable workflow orchestration layer owned by the gateway.

## Relationship To Causal Chain

This plan must not replace, fork, or compete with the causal chain.

The intended separation is:

- causal chain = immutable audit log of what happened
- workflow store = mutable operational state of what should happen next
- session timeline / CLI graph = human-facing projections built from those sources

### Causal chain remains authoritative for audit

The causal chain already provides:

- append-only, hash-linked, tamper-evident records
- cross-layer trace correlation via `session_id`, `turn_id`, and `event_seq`
- durable audit for gateway actions, agent actions, approvals, and tool results

Nothing in this plan should weaken or bypass that.

### Workflow store is an orchestration index, not an audit replacement

`WorkflowRun`, `TaskRun`, and `WorkflowEventRecord` should be treated as:

- mutable runtime coordination state
- resumable execution metadata
- efficient query surfaces for planner/task orchestration

They should not be treated as the sole historical truth for forensics or compliance.

### Required invariants

- every important workflow transition should still emit causal-chain entries
- workflow events may mirror orchestration transitions, but they do not replace causal entries
- CLI graph should prefer workflow events for live state, while preserving links back to causal-chain evidence when available
- timeline generation may combine workflow state and causal entries, but causal chain remains the immutable ground truth for audit

### Collision to avoid

Do not introduce a second "authoritative event history" with semantics that can drift from the causal chain.

If both exist:

- workflow events should optimize orchestration and UX
- causal chain should optimize auditability and evidence

That layering is compatible with the current architecture and avoids conceptual collision.

### Implementation note (causal mirror)

Important workflow transitions **also** emit best-effort gateway causal-chain rows (same append-only log as today; category `gateway`) so audit and orchestration stay **correlatable** without making the workflow JSONL authoritative for forensics:

| Causal `action` | When |
|-----------------|------|
| `workflow.started` | New `WorkflowRun` created for a root session |
| `workflow.task.spawned` | `agent.spawn` persisted a `TaskRun` and workflow `task.spawned` event |
| `workflow.task.completed` / `workflow.task.failed` | Task status updated to terminal (or `workflow.task.awaiting_approval` / `workflow.task.updated` for other updates) |

Payloads include `workflow_id` and `task_id` where applicable. Rows are indexed under the workflow **root** `session_id` so the lead session’s evidence trail shows delegation.

Code: `autonoetic-gateway/src/scheduler/workflow_causal.rs` (calls `log_gateway_causal_event`).

### 1. WorkflowRun

One root workflow per user-facing task.

Suggested fields:

- `workflow_id`
- `root_session_id`
- `lead_agent_id`
- `status`
- `created_at`
- `updated_at`
- `current_planner_checkpoint`
- `active_task_ids`
- `blocked_task_ids`
- `pending_approval_ids`
- `resume_policy`
- `event_cursor`

### 2. TaskRun

Each delegated child execution becomes a durable task record.

Suggested fields:

- `task_id`
- `workflow_id`
- `parent_task_id` or `spawned_by_turn_id`
- `agent_id`
- `session_id`
- `status`
- `inputs_ref`
- `result_ref`
- `checkpoint_ref`
- `depends_on`
- `blocks_planner`
- `join_group`
- `approval_ids`
- `attempt`

### 3. ApprovalBarrier

Approval should bind to tasks and workflows, not to inferred session names.

Suggested fields:

- `approval_id`
- `workflow_id`
- `task_id`
- `kind`
- `status`
- `subject`
- `requested_at`
- `resolved_at`
- `resume_target`

### 4. WorkflowEvent

All orchestration state changes should emit durable workflow events.

Examples:

- `workflow.started`
- `planner.checkpointed`
- `task.spawned`
- `task.started`
- `task.awaiting_approval`
- `approval.resolved`
- `task.resumed`
- `task.completed`
- `task.failed`
- `planner.resumed`
- `planner.completed`
- `workflow.completed`

This event stream becomes the source for:

- CLI live graph
- CLI timeline/follow views
- durable session timelines
- future web monitoring

## State Machines

### WorkflowRun Status

- `Active`
- `WaitingChildren`
- `BlockedApproval`
- `Resumable`
- `Completed`
- `Failed`
- `Cancelled`

### TaskRun Status

- `Pending`
- `Runnable`
- `Running`
- `AwaitingApproval`
- `Paused`
- `Succeeded`
- `Failed`
- `Cancelled`

### Planner Lifecycle Rule

The planner is not "done" when it delegates.

Instead:

1. planner turn executes
2. gateway checkpoints planner state
3. planner enters `WaitingChildren`
4. child tasks execute independently
5. gateway resumes planner only when its join condition is satisfied

This keeps planner continuity durable without requiring a live process.

## Execution Semantics

### A. `agent.spawn` becomes asynchronous

This is the key change needed for robust parallelism.

Current behavior is effectively synchronous:

- planner calls `agent.spawn`
- gateway waits for the child to finish or block
- planner only then continues the turn

This makes true parallel delegation impossible and is exactly why one approval can derail the whole turn.

New behavior:

- `agent.spawn` creates a `TaskRun`
- returns immediately with `task_id`, `session_id`, and accepted metadata
- gateway scheduler starts the child task independently
- planner can issue several `agent.spawn` calls in one turn

The planner should reason in terms of:

- "spawn these tasks"
- "wait for these tasks"
- "resume me when this join condition is satisfied"

### B. Parallel child execution

The planner must be able to spawn several children in parallel in one turn.

Examples:

- `researcher.default` and `architect.default` in parallel
- `evaluator.default` and `auditor.default` in parallel
- several coders exploring different branches in parallel

The gateway should support:

- launching multiple `Runnable` tasks concurrently
- configurable concurrency limits
- per-agent admission limits
- per-workflow concurrency limits

### C. Join semantics

Planner resume should be driven by explicit join conditions, not by whichever child happened to wake.

Suggested join policies:

- `all_of(task_ids)`
- `any_of(task_ids)`
- `first_success(task_ids)`
- `quorum(task_ids, n)`
- `manual` for workflows that want explicit planner wake on every child completion

For the initial implementation, `all_of(task_ids)` is enough and already solves the current issue cleanly.

### D. Approval as a task barrier

When a child task hits approval:

- that task becomes `AwaitingApproval`
- the workflow records the approval barrier
- other unrelated child tasks may continue running
- the planner remains checkpointed in `WaitingChildren`

When approval is granted:

- the gateway resolves the barrier
- all newly unblocked tasks become `Runnable`
- multiple children can resume in parallel if several were waiting

Approval should not directly resume the planner unless the planner join condition is now satisfied.

### E. Child completion resumes planner

The planner should resume because its waiting condition is satisfied, not because a raw signal happened.

That gives the clean control flow:

1. planner checkpoints
2. children run
3. some children may block on approval
4. approvals unblock those tasks
5. children finish
6. gateway sees join condition satisfied
7. planner resumes with durable child outputs

## Planner Interface Changes

The planner should not need to understand session plumbing.

Suggested gateway-facing primitives:

- `agent.spawn(...) -> { task_id, session_id, accepted: true }`
- `workflow.wait({ task_ids, policy })`
- `workflow.get_status({ workflow_id })`
- `workflow.get_results({ task_ids })`

The planner prompt can keep the current delegation style, but the underlying runtime should stop assuming:

- "spawn means wait for child completion"
- "approval means tell the user and end the planner turn forever"

## Approval Targeting

Stop deriving parent/child semantics from session-id string shapes like `root/child`.

At approval request creation time, store explicit metadata:

- `workflow_id`
- `task_id`
- `root_session_id`
- `lead_agent_id`
- `resume_targets`

Then approval resolution can deterministically:

- unblock the correct task
- emit workflow events for the root workflow
- notify all subscribed views

## Durability and Restart Model

All orchestration-critical state should survive:

- gateway restart
- planner process exit
- child process exit
- CLI disconnect

The minimal durable stores are:

- workflow store
- task store
- approval store
- checkpoint store
- event log

Agent runtimes become disposable workers that can be restarted from task state.

## Checkpoint Compatibility Review

The current codebase already has three related but distinct continuity mechanisms:

- `SessionContext`: thin same-session prompt continuity for normal re-spawn/wake
- `SessionSnapshot` + `session.fork`: history capture and branching into a new session
- `sdk.state.checkpoint`: script-managed local progress checkpoint

These are useful building blocks, but they are not sufficient as the primary workflow pause/resume mechanism.

### What exists today

- Normal same-session continuation rebuilds continuity from `SessionContext`
- `session.fork` creates a new branched session from a saved snapshot
- scripts can opt into `sdk.state.checkpoint()` / `sdk.state.get_checkpoint()`

### Why this is not enough

- `SessionContext` is intentionally thin and prompt-oriented, not deterministic execution state
- `SessionSnapshot` is for branching and replay, not "pause this workflow and resume it later in place"
- the current SDK checkpoint is too low-level and too local to be the orchestration backbone
- the current SDK checkpoint is not a workflow/task-scoped checkpoint model

### Required checkpoint task

Add a real workflow/task checkpoint layer on top of the current continuity mechanisms.

Suggested properties:

- checkpoint scope must be `workflow_id` + `task_id` or equivalent durable identifiers
- planner checkpoints must store enough state to resume waiting/join logic deterministically
- child task checkpoints must store enough state to retry or resume after approval boundaries
- workflow resume must not depend on session fork
- workflow resume must not rely only on prompt reconstruction from `SessionContext`

### Relationship to existing mechanisms

- keep `SessionContext` for lightweight conversational continuity
- keep `SessionSnapshot` for user-visible branching/forking and debugging
- keep `sdk.state.checkpoint` for agent-authored script progress
- add workflow/task checkpoints as the orchestration-grade resume primitive

This gives us a clean layering model:

- prompt continuity: `SessionContext`
- branch/fork continuity: `SessionSnapshot`
- local script progress: `sdk.state.checkpoint`
- orchestration pause/resume: workflow/task checkpoints

## CLI Real-Time Graph

`timeline.md` is a very good artifact, but it is retrospective and file-based.

We also want a live workflow view in the CLI.

### Desired UX

While using `autonoetic chat`, the user can see the workflow graph update in real time:

- planner node
- child task nodes
- approval barrier nodes
- edges for spawn, waiting, blocking, completion, and resume
- current status colors

Suggested statuses:

- running
- waiting
- blocked on approval
- resumed
- completed
- failed

### Data source

The graph should not parse `timeline.md`.

It should subscribe to the durable `WorkflowEvent` stream.

That same stream can also feed:

- `trace follow`
- a future web UI
- session timeline generation

### CLI integration options

Option 1:

- add a graph pane to `autonoetic chat`
- split view: conversation on the left, workflow graph or event rail on the right

Option 2:

- add `autonoetic trace graph <session_id> --follow`
- keep chat and graph as separate tools initially

Recommended rollout:

- implement `trace graph --follow` first
- then embed the same component into `autonoetic chat`

### Graph rendering model

Start simple.

Phase 1:

- tree or DAG-like text view
- one line per node
- indentation for parent/child relationships
- symbols for status

Example:

```text
workflow demo-session-1 [waiting_children]
planner.default [checkpointed]
|- coder.default#task-1 [awaiting_approval apr-c922c96f]
|- architect.default#task-2 [pending]
approval apr-c922c96f [approved]
coder.default#task-1 [resumed]
coder.default#task-1 [completed]
planner.default [resumed]
```

Phase 2:

- richer TUI graph layout
- selection and drill-down
- filters by status, agent, or task group

## Relationship With Existing Trace and Timeline Tools

Existing assets are already strong and should be reused:

- `timeline.md` remains the durable human-readable narrative
- `trace follow` remains a low-level event feed
- `trace rebuild` remains useful for postmortems

The new workflow layer should sit above them:

- timeline rows are generated from workflow events plus runtime events
- CLI graph renders workflow events live
- trace commands can optionally show workflow IDs, task IDs, and approval IDs

## Implementation Phases

### Progress checklist (update as sub-tasks land)

| ID | Done | Description |
|----|------|-------------|
| **P1.1** | [x] | Shared types: `WorkflowRun`, `TaskRun`, `WorkflowEventRecord`, status enums (`autonoetic-types::workflow`) |
| **P1.2** | [x] | Durable store under `.gateway/scheduler/workflows/` (root index → `workflow_id`, `runs/<id>/workflow.json`, `tasks/`, `events.jsonl`) |
| **P1.3** | [x] | Wire `agent.spawn` to ensure workflow per root session and return `workflow_id` + `task_id` |
| **P1.4** | [x] | Emit `task.spawned` / `task.completed` (and related) from the spawn path _(sync spawn: success → `task.completed`; error → `task.failed` via store)_ |
| **P1.5** | [x] | Mirror key workflow/task transitions into the gateway **causal chain** (`workflow_causal` + `log_gateway_causal_event`) per § Relationship To Causal Chain |
| **P6.1** | [x] | Read-only CLI: `autonoetic trace workflow <id> [--root] [--follow] [--json]` reads `events.jsonl`; gateway `resolve_workflow_id_for_root_session`, `load_workflow_events` (`workflow_store`) |
| **P7.1** | [x] | `autonoetic trace graph` (root session or `wf-…`) `[--follow] [--json]` — text tree from `workflow.json` + `tasks/*.json` + tail of `events.jsonl`; `list_task_runs_for_workflow` |
| **P6.2** | [x] | `timeline.md`: append rows for gateway `workflow.*` mirrors + `workflow_graph.md` beside timeline (event-driven refresh on `append_workflow_event`) |
| **P2.1** | [x] | `QueuedTaskRun` type, `JoinPolicy` enum, `join_group` field on `TaskRun`, `queued_task_ids`/`join_policy`/`join_task_ids` on `WorkflowRun` (`autonoetic-types::workflow`) |
| **P2.2** | [x] | Queue persistence: `enqueue_task`, `dequeue_task`, `load_queued_tasks`, `load_all_queued_tasks`, `check_join_condition` (`workflow_store`) |
| **P2.3** | [x] | Scheduler tick: `process_queued_workflow_tasks` picks up queued tasks, creates TaskRun, spawns background `spawn_agent_once`, updates status on completion (`scheduler.rs`) |
| **P2.4** | [x] | `agent.spawn` async mode: `async: true` parameter → enqueues `QueuedTaskRun`, returns immediately with `task_id` + `accepted: true` |
| **P2.5** | [x] | `workflow.wait` tool: checks task statuses by `task_ids`, returns per-task status + `join_satisfied` flag; registered in `default_registry` |
| **P4.1** | [x] | Join condition check wired into `update_task_run_status`: after terminal task update, checks if all `join_task_ids` are done → marks workflow `Resumable` + emits `workflow.join.satisfied` event |
| **P4.2** | [x] | `workflow.wait` blocking mode: `timeout_secs > 0` polls task status in a loop until join satisfied or timeout |
| **P4.3** | [x] | `WorkflowJoinSatisfied` signal type + `send_workflow_join_satisfied` writes signal to planner session directory on join satisfaction |
| **P5.1** | [x] | `workflow_id` and `task_id` optional fields added to `ApprovalRequest` and `ApprovalDecision` |
| **P5.2** | [x] | Approval requests created by `sandbox.exec` and `agent.install` resolve and populate `workflow_id` from session context |
| **P5.3** | [x] | `unblock_task_on_approval` called on approve/reject — updates task status (Runnable on approve, Failed on reject) + emits workflow events |
| **P6.1** | [x] | `WorkflowEventStream` — in-process subscription via `std::sync::mpsc` channel, polls `events.jsonl` on a background thread |
| **P6.2** | [x] | `workflow_updated_since` — fast check if any workflow events were emitted after a timestamp |
| **P7.1** | [x] | `compact_workflow_summary` — single-line summary of workflow state (running/done/failed/queued counts) |
| **P7.2** | [x] | Lifecycle integration: injects `[workflow status] ...` as a system message at turn end (EndTurn/StopSequence) |
| **P8.1** | [x] | Planner SKILL.md updated with parallel delegation (async spawn + workflow.wait) guidance |
| **P8.2** | [x] | Approval section updated: async spawn not blocked by pending approvals |
| **P3.1** | [x] | `WorkflowCheckpoint` type + `checkpoint_planner()` with auto-versioning |
| **P3.2** | [x] | `TaskCheckpoint` type + `checkpoint_task()` with auto-versioning |
| **P3.3** | [x] | Persistence under `runs/<wf_id>/checkpoints/` + checkpoint events emitted |
| **P3.4** | [x] | `load_workflow_checkpoint`, `load_task_checkpoint`, `load_all_task_checkpoints` |
| **P3.5** | [x] | 4 new tests: planner roundtrip, task roundtrip, join state capture, event emission |
| **P3.6** | [x] | `checkpoint_planner` wired into lifecycle at turn end (EndTurn/StopSequence) |
| **P3.7** | [x] | `checkpoint_task` wired into scheduler `process_queued_workflow_tasks` (start/complete/fail) |

### Phase 1: Durable workflow store

- add `WorkflowRun`, `TaskRun`, and `WorkflowEvent` stores
- assign a `workflow_id` to every root chat/planner task
- persist explicit root/child relationships

### Phase 2: Async child spawning ✅

- `agent.spawn(async: true)` creates a `QueuedTaskRun` and returns immediately with `task_id`
- Scheduler tick (`process_queued_workflow_tasks`) picks up queued tasks, spawns `spawn_agent_once` in background tokio tasks
- `workflow.wait(task_ids)` tool lets the planner check task completion status (non-blocking)
- Child outputs persisted in task result records on completion/failure
- **Remaining:** Planner resume on join condition satisfaction (Phase 4 integration — currently `workflow.wait` is manual; planner must call it explicitly)

### Phase 3: Workflow/task checkpoints ✅

- `WorkflowCheckpoint` type: durable planner-level state (intent, pending tasks, join policy, arbitrary context) ✅
- `TaskCheckpoint` type: durable task-level state (step label, arbitrary execution state) ✅
- `checkpoint_planner()` / `checkpoint_task()` convenience functions with auto-incrementing versions ✅
- Persistence under `runs/<wf_id>/checkpoints/` (planner.json + per-task JSON files) ✅
- Checkpoint events emitted to `events.jsonl` (`workflow.checkpoint.saved`, `task.checkpoint.saved`) ✅
- Separately layered from `SessionContext`, `SessionSnapshot`, and `sdk_checkpoint` per the layering model ✅
- `checkpoint_planner` wired into lifecycle at turn end (EndTurn/StopSequence) — captures assistant message as intent ✅
- `checkpoint_task` wired into scheduler `process_queued_workflow_tasks` — checkpoints at start, completion, and failure ✅

### Phase 4: Planner checkpoint and wait ✅

- `update_task_run_status` checks join condition after terminal task updates ✅
- When all `join_task_ids` reach terminal status → workflow marked `Resumable` + `workflow.join.satisfied` event emitted ✅
- `WorkflowJoinSatisfied` signal written to planner session directory for signal poller delivery ✅
- `workflow.wait` supports blocking mode (`timeout_secs > 0`) — polls until join satisfied or timeout ✅
- **Remaining:** Signal poller delivery of `WorkflowJoinSatisfied` requires a running gateway with signal poller active; the planner session must be listening for `event.ingest` to pick it up. `workflow.wait` blocking mode is the reliable fallback.

### Phase 5: Approval barriers ✅

- `ApprovalRequest` and `ApprovalDecision` now carry `workflow_id` and `task_id` (optional) ✅
- `sandbox.exec` and `agent.install` resolve `workflow_id` from the current session when creating approval requests ✅
- On approval resolution (approve or reject), `unblock_task_on_approval` updates the blocked task's status:
  - Approved → `Runnable` (task can resume execution)
  - Rejected → `Failed` (task is marked failed, join condition checks it) ✅
- **Remaining:** `task_id` population requires the async spawn path to pass the task context to the child session; currently `task_id` is `None` for sync sandbox.exec approvals (they don't block an async task)

### Phase 6: Event stream ✅

- `WorkflowEventStream` struct: in-process subscription via `std::sync::mpsc` channel, polls `events.jsonl` on a background thread ✅
- `workflow_updated_since(config, root_session_id, timestamp)`: fast check if workflow events were emitted after a given timestamp ✅
- CLI `trace workflow --follow` already works via the existing polling mechanism ✅

### Phase 7: CLI graph embed in chat ✅

- `compact_workflow_summary(config, root_session_id)`: single-line summary like `wf-abc123 · 2 running · 1 done` ✅
- Lifecycle integration: injects `[workflow status] ...` as a system message at turn end (EndTurn/StopSequence) ✅
- Planner sees delegated task progress on its next wake without manual polling

### Phase 8: Cleanup and migration ✅

- Planner SKILL.md updated with parallel delegation (async spawn + workflow.wait) guidance ✅
- Approval section updated: async spawn not blocked by pending approvals ✅
- Session-path inference: `root_session_id()` still used in places; full cleanup deferred (non-breaking, backward-compatible)

## Testing Plan

### Backend orchestration tests

- planner spawns two children in parallel and both run
- one child blocks on approval while another completes
- approval resumes only the blocked child
- planner resumes only after the configured join condition is satisfied
- two blocked children can resume in parallel after approval resolution
- gateway restart preserves workflow state and resumes correctly

### CLI tests

- `trace graph --follow` shows live task creation and completion
- chat graph pane updates while the workflow is running
- approval barrier appears live
- planner resume appears live
- no manual `continue` is required for normal workflow continuation

### Regression tests for the current failure

Reproduce the exact `demo-session-1` scenario:

- planner spawns coder and architect in parallel
- coder hits approval
- planner checkpoints into `WaitingChildren`
- approval resolves coder task
- coder completes
- planner resumes and continues to architect or next step
- CLI graph and chat both reflect the continuation in real time

## Success Criteria

- approval no longer "kills" the planner workflow
- planners can delegate several child agents in parallel
- several approval-blocked children can resume in parallel
- planner continuation is driven by workflow joins, not raw signal delivery
- the CLI shows live workflow state without requiring `timeline.md`
- `timeline.md` and CLI graph are projections of the same durable event model

## Recommended First Slice

The smallest meaningful vertical slice is:

1. add `workflow_id` and `task_id` ✅
2. make `agent.spawn` asynchronous ✅ (`async: true` + `workflow.wait`)
3. add workflow/task checkpoints ✅ (`WorkflowCheckpoint`, `TaskCheckpoint`, `checkpoint_planner()`, `checkpoint_task()`, versioned persistence)
4. checkpoint planner into `WaitingChildren` ✅ (join condition + signal on satisfaction)
5. bind approvals to `task_id` ✅ (`workflow_id`/`task_id` on `ApprovalRequest` + `unblock_task_on_approval`)
6. resume planner on child completion ✅ (`workflow.wait` blocking + `WorkflowJoinSatisfied` signal)
7. add a simple `trace graph --follow` text view ✅

That slice is enough to fix the current architectural problem and gives us a clean base for richer graph UX later.
