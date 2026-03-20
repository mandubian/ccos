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

### Phase 1: Durable workflow store

- add `WorkflowRun`, `TaskRun`, and `WorkflowEvent` stores
- assign a `workflow_id` to every root chat/planner task
- persist explicit root/child relationships

### Phase 2: Async child spawning

- change `agent.spawn` to create tasks and return immediately
- launch child tasks from the scheduler
- persist child outputs in task result records

### Phase 3: Planner checkpoint and wait

- checkpoint planner at the end of delegation turns
- add `WaitingChildren` and join semantics
- resume planner from checkpoint when join conditions are satisfied

### Phase 4: Approval barriers

- bind approvals to `workflow_id` and `task_id`
- on approval, unblock tasks instead of "waking sessions"
- support multiple approvals resolving multiple tasks concurrently

### Phase 5: Event stream

- emit durable `WorkflowEvent` records for orchestration changes
- expose a live follow API or local event stream reader
- keep `timeline.md` as a projection of the same events

### Phase 6: CLI graph

- add `autonoetic trace graph <session_id> --follow`
- render planner, children, approvals, and state transitions live
- embed the same graph/event rail into `autonoetic chat`

### Phase 7: Cleanup and migration

- remove remaining session-path inference for orchestration
- reduce approval signal files to a compatibility or transport layer
- update planner guidance to describe async spawn + wait behavior

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

1. add `workflow_id` and `task_id`
2. make `agent.spawn` asynchronous
3. checkpoint planner into `WaitingChildren`
4. bind approvals to `task_id`
5. resume planner on child completion
6. add a simple `trace graph --follow` text view

That slice is enough to fix the current architectural problem and gives us a clean base for richer graph UX later.
