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

## Recommended Fix Direction

### Minimal practical fix

1. Keep signal files until a durable resume is confirmed.
2. Update `chat.rs` so an approval signal triggers an explicit resume request on the chat's own connection.
3. After that request returns, render the assistant reply as a normal assistant message.
4. Only then consume the signal file.

This would solve the immediate user-visible problem:

- the TUI would continue displaying messages after approval

### Important consistency fix

Make direct and fallback delivery use the exact same structured payload.

The fallback path should deliver the same `approval_resolved` JSON message as the direct path.

### Correctness fixes

- update approval ID extraction to support `apr-xxxxxxxx`
- gate background approval wake with `background.wake_predicates.approval_resolved`
- review parent/child signal cleanup symmetry

### Stronger architectural fix

Move signal polling and resume delivery ownership into the long-running gateway process rather than the short-lived approval CLI process.

That would make wake behavior durable and reduce process lifetime races.

## Suggested Follow-Up Work

- patch `autonoetic/src/cli/chat.rs` so signals trigger a real resume request
- patch `autonoetic-gateway/src/scheduler/signal.rs` to send structured `approval_resolved` payloads
- stop chat from deleting signals before resume success
- add E2E coverage for:
  - chat open
  - approval resolved externally
  - planner resumes
  - assistant output appears automatically in chat

