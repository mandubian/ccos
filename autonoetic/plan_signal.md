# Approval + Signal Investigation

## Scope

This note captures the current understanding of the approval and signal mechanism in the autonoetic gateway, with emphasis on:

- how an approval is created and resolved
- how approval resolution is supposed to trigger a signal
- how that signal is expected to wake a waiting agent
- how `autonoetic chat` is expected to continue showing messages
- why the current signal behavior appears unreliable

## Key Files

- `autonoetic/autonoetic-gateway/src/runtime/tools.rs`
- `autonoetic/autonoetic-gateway/src/scheduler/approval.rs`
- `autonoetic/autonoetic-gateway/src/scheduler/signal.rs`
- `autonoetic/autonoetic-gateway/src/scheduler/decision.rs`
- `autonoetic/autonoetic-gateway/src/scheduler/runner.rs`
- `autonoetic/autonoetic-gateway/src/router.rs`
- `autonoetic/autonoetic-gateway/src/server/jsonrpc.rs`
- `autonoetic/autonoetic/src/cli/chat.rs`
- `autonoetic/autonoetic/src/cli/gateway.rs`
- `autonoetic/agents/lead/planner.default/SKILL.md`
- `autonoetic/autonoetic-gateway/tests/agent_install_approval_e2e.rs`
- `autonoetic/autonoetic-gateway/tests/background_scheduler_integration.rs`

## Current End-to-End Flow

### 1. Approval request creation

Approvals are created by runtime tools when a gated action requires operator authorization.

- `agent.install` creates a pending `ApprovalRequest`
- `sandbox.exec` creates a pending `ApprovalRequest` for remote access
- pending approvals are stored under:
  - `.gateway/scheduler/approvals/pending/<request_id>.json`
- install payloads are also stored for deterministic retry:
  - `.gateway/scheduler/approvals/pending/<request_id>_payload.json`

Important detail:

- approval IDs are now short IDs like `apr-xxxxxxxx`
- they are no longer UUID-shaped

### 2. Approval decision

When the operator runs the CLI approval command:

- `autonoetic/src/cli/gateway.rs` calls `autonoetic_gateway::scheduler::approve_request()`
- `approve_request()` delegates to `decide_request()`
- the decision is written under:
  - `.gateway/scheduler/approvals/approved/<request_id>.json`
- pending approval files are removed or transitioned out of `pending`

### 3. Direct execution after approval

After approval:

- `agent.install` may be auto-executed immediately from the stored payload
- `sandbox.exec` is not auto-executed in the same way; the waiting agent is expected to retry with `approval_ref`

### 4. Session resume after approval

After the decision is recorded, `resume_session_after_approval()` tries to notify the waiting session.

It currently does three things:

1. Builds a structured synthetic message:
   - JSON string with `type: "approval_resolved"`
2. Writes a signal file:
   - `.gateway/signal/<session_id>/<request_id>.json`
3. Spawns a thread that connects back to the gateway JSON-RPC server and sends a synthetic `event.ingest`

For `agent.install`:

- signal is written to the child session
- signal is also written to the parent planner session
- the direct JSON-RPC resume targets the parent session

For other actions:

- direct JSON-RPC resume targets the original session

### 5. Gateway routing

The synthetic resume request is routed through `router.rs`:

- method: `event.ingest`
- if `target_agent_id` is omitted, the router resolves the target from the session lead binding
- otherwise it falls back to `default_lead_agent_id`

This means approval resume depends on the correct session-to-lead-agent binding being present.

### 6. CLI chat behavior

`autonoetic/src/cli/chat.rs` has two relevant behaviors:

- it renders JSON-RPC responses that match request IDs it sent itself
- it separately polls `.gateway/signal/<session_id>/` every few seconds

When a signal is found:

- the TUI adds a `Signal` message such as `Approval approved for <agent>`
- the TUI then deletes the signal file with `consume_signal()`

It does not currently:

- inject a resume request into the gateway on behalf of the chat session
- fetch the assistant reply produced by a resumed planner turn
- display unsolicited assistant replies from another connection

## Important Distinction: Background Wake vs Live Session Resume

There are two different mechanisms in play.

### Background scheduler wake

For background agents:

- `scheduler/decision.rs::should_wake()` checks approved decision files
- if an open approval request becomes approved, it returns `WakeReason::ApprovalResolved`
- `scheduler/runner.rs::handle_due_wake()` resumes the background action on the next scheduler tick

This path does not depend on the signal file mechanism used by the CLI chat.

### Live session resume

For chat/planner sessions:

- `resume_session_after_approval()` writes a signal file
- it also tries immediate resume via a synthetic `event.ingest`

This path is the one that appears unreliable in practice.

## Confirmed Findings

### Finding 1: the approval CLI process is too short-lived to be a reliable wake source

The approval CLI command calls `approve_request()`, which:

- writes signal files
- starts the signal poller
- spawns a background thread to send the synthetic resume event

But all of that is initiated from the short-lived CLI process that exits immediately after printing success.

This makes the wake path non-durable.

Practical consequence:

- the notification thread and signal poller can die with the process
- the resume attempt may never complete
- the CLI text itself already suggests the current workaround:
  - return to chat and type `continue` or `done`

### Finding 2: `autonoetic chat` is not a true streaming/subscription client

The TUI only displays:

- responses to requests sent on its own socket
- local signal banners

If the gateway resumes a planner turn using a separate internal JSON-RPC connection, the resulting assistant reply goes back to that helper connection, not to the user's existing TUI connection.

Practical consequence:

- the planner may resume successfully
- the TUI may still remain silent

### Finding 3: the TUI consumes signal files before a durable resume is confirmed

`check_signals()` in the TUI:

- reads pending signals
- shows a banner
- deletes the signal file immediately

At the same time, the gateway signal poller also uses those same files as fallback delivery state.

Practical consequence:

- if immediate resume fails
- and the TUI sees the signal first
- the fallback delivery state is lost

This is a real race.

### Finding 4: direct resume and fallback signal delivery do not send the same payload

Direct resume from `approval.rs` sends a structured JSON string:

```json
{
  "type": "approval_resolved",
  "request_id": "...",
  "agent_id": "...",
  "status": "approved",
  "install_completed": true,
  "message": "..."
}
```

Fallback delivery from `signal.rs::deliver_signal()` sends only:

- the human-readable `message` string
- some metadata fields

Practical consequence:

- the planner prompt expects structured `approval_resolved` content
- fallback delivery is semantically weaker than the direct path
- behavior can diverge depending on which path was used

### Finding 5: chat-side approval ID extraction is stale

`extract_approval_request_id()` in `autonoetic/src/cli/chat.rs` still looks for UUID-shaped request IDs.

But current approval IDs are short IDs like:

- `apr-1234abcd`

Practical consequence:

- the TUI may fail to detect and display approval request IDs from assistant replies

### Finding 6: background approval wake appears to bypass its config flag

In `scheduler/decision.rs::should_wake()`:

- approved approval files are checked before `background.wake_predicates.approval_resolved`

Practical consequence:

- background wake on approval appears effectively unconditional once the request is open and approved
- the config flag is not truly gating behavior in that path

### Finding 7: parent/child signal handling is asymmetric

For `agent.install`, approval resolution may:

- write a signal to the child session
- write a signal to the parent planner session
- send the direct resume only to the parent session

But signal cleanup after successful send is not symmetrical across both files.

Practical consequence:

- one session may keep a stale signal
- duplicate or confusing resume handling is possible

### Finding 8: session targeting is brittle

Some logic derives context from session ID shape, for example:

- parent session is extracted by splitting on `/`
- `SandboxExec` derives an `agent_id` by parsing the last path segment

Practical consequence:

- if session naming conventions evolve
- resume events may target the wrong place or carry degraded context

## Expected vs Actual Behavior

### Expected behavior

After operator approval:

1. the waiting planner or child agent wakes automatically
2. the approval result is delivered in a structured form
3. the planner continues the workflow
4. `autonoetic chat` continues showing the resumed assistant output without user intervention

### Actual behavior observed in the code

After operator approval:

1. decision is recorded correctly
2. signal file is written
3. direct resume may or may not be sent reliably
4. background wake works on scheduler tick
5. chat TUI often only shows a signal banner
6. user may still need to type another message to see planner continuation

## Tests and Validation

The following tests were run successfully:

- `cargo test -p autonoetic-gateway --test agent_install_approval_e2e -- --nocapture`
- `cargo test -p autonoetic-gateway --test background_scheduler_integration test_background_scheduler_evolution_flow_through_public_api -- --nocapture`

What this confirms:

- backend approval creation and approval validation are working
- background wake after approval is working through the scheduler path

What is not currently covered by those tests:

- approve in one terminal while `autonoetic chat` is already open in another terminal
- verify that resumed planner output is automatically displayed in the existing TUI session

That missing E2E coverage is likely why this regression area remained unnoticed.

## Planner Prompt Expectations

`autonoetic/agents/lead/planner.default/SKILL.md` explicitly documents handling of structured `approval_resolved` messages.

It also still instructs the user:

- after approval, return to chat and type `continue` or `done`

This is a strong signal that the system currently relies on manual chat re-entry as a fallback workflow.

## Root Cause Summary

The current issue is not one single bug. It is a design mismatch between:

- backend approval resolution
- wake signaling
- chat UI display behavior

The most likely composite failure mode is:

1. approval is correctly recorded
2. signal file is written
3. direct resume is launched from a short-lived CLI process
4. the resumed planner reply is produced on another connection
5. the TUI does not receive that assistant reply
6. the TUI may consume the signal file before fallback delivery can use it
7. user experiences this as "signal does not work" or "agent did not wake"

## Recommended Architecture (Single Durable Model)

Use one architecture and remove split behavior between "direct resume" and "fallback signal":

1. **Gateway-owned delivery**: the long-running gateway daemon owns approval notification delivery.
2. **One payload contract**: every approval resolution uses the same structured `approval_resolved` payload.
3. **Durable pending state + explicit ack**: notification files/records are kept until delivery is acknowledged.
4. **Session-safe targeting**: approval targets are explicit metadata, not inferred from session ID string parsing.
5. **Chat resumes on its own connection**: the TUI triggers resume itself and only consumes signal after success.

## Target Behavior

After operator approval (or rejection):

1. decision is durably recorded in approvals store
2. gateway creates pending notification record(s) for the intended target(s)
3. gateway delivery loop attempts delivery until success
4. for chat sessions, TUI receives pending notification, sends resume on its own socket, renders assistant reply
5. notification is consumed only after ack/success
6. background scheduler wake on approval only happens when `wake_predicates.approval_resolved` is enabled

This model keeps behavior simple while being robust to process lifetimes, races, and client disconnects.

## Concrete Design

### A) Approval decision remains source of truth

- Keep `ApprovalDecision` in `.gateway/scheduler/approvals/{approved|rejected}/`.
- Treat this as canonical state.
- Do not rely on transient CLI process execution for actual wake delivery guarantees.

### B) Gateway-owned notification queue

- Introduce a durable pending-notification mechanism under gateway-owned storage (signal dir can be reused if desired).
- Each notification record should include:
  - `request_id`
  - `status` (`approved` or `rejected`)
  - structured `approval_resolved` payload
  - explicit target (`session_id`, optional `target_agent_id`, optional channel target)
  - delivery/ack metadata (`created_at`, `attempt_count`, `last_attempt_at`, `acked_at`)
- Start and own the poll/delivery loop in gateway server startup, not from `gateway approvals approve` CLI path.

### C) One payload shape everywhere

Both immediate and retry delivery paths must send the same structured payload:

```json
{
  "type": "approval_resolved",
  "request_id": "...",
  "agent_id": "...",
  "status": "approved",
  "install_completed": true,
  "message": "..."
}
```

No plain-text-only fallback path should exist for agent-facing delivery.

### D) Chat consumption and ack semantics

In `autonoetic chat`:

- Detect pending approval notification for current session.
- Display a signal banner.
- Trigger `event.ingest` on the same chat connection using the structured payload.
- Render returned `assistant_reply` immediately.
- Ack/consume signal only after successful resume+render.
- If resume fails, keep notification pending for retry (do not consume).

### E) Background wake semantics

- In `scheduler/decision.rs::should_wake()`, honor `background.wake_predicates.approval_resolved` before returning `WakeReason::ApprovalResolved`.
- Keep current deterministic behavior for approved scheduled actions, but only when wake predicate allows it.

### F) Targeting model

- Stop deriving parent/child targeting from session ID splitting heuristics.
- Persist explicit target context at approval request creation (or in an associated metadata record), then use that explicit target at resolution time.

## Why this is the best single design

- **Simple mental model**: one owner (gateway), one payload contract, one durable queue.
- **Robustness**: no dependency on short-lived CLI process threads.
- **Correctness**: no race where chat consumes signal before durable resume is confirmed.
- **Consistency**: planner receives identical structured semantics regardless of delivery attempt path.
- **Extensibility**: human channels (TUI now, WhatsApp-like later) and agent channels share the same approval-resolution event model.

## Implementation Checklist

1. `autonoetic-gateway/src/server/mod.rs`
   - start gateway-owned signal/notification poller at daemon startup
2. `autonoetic-gateway/src/scheduler/approval.rs`
   - stop spawning resume thread from CLI-owned call path
   - create durable notification records instead
3. `autonoetic-gateway/src/scheduler/signal.rs`
   - enforce single structured payload delivery
   - add ack-aware consume semantics
4. `autonoetic/src/cli/chat.rs`
   - support `apr-xxxxxxxx` extraction
   - on signal: call `event.ingest` via current socket, render reply, then ack/consume
5. `autonoetic-gateway/src/scheduler/decision.rs`
   - gate approval wake by `background.wake_predicates.approval_resolved`
6. tests
   - add E2E: open chat, approve in separate terminal, planner resumes, assistant reply appears automatically
   - add race-resilience test: signal is not consumed when resume fails

