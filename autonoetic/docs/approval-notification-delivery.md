# Approval Notification Delivery

This document describes the current approval-resolution delivery model in Autonoetic.

## Summary

Approval is a first-class control surface. When an operator approves or rejects a request:

1. the decision is written durably under the scheduler approvals store,
2. a structured approval-resolution notification is persisted for the target session(s),
3. delivery is handled by long-running gateway-owned components and channel consumers,
4. the notification remains pending until explicitly consumed.

This model replaces short-lived CLI-owned wake logic with durable gateway-owned delivery.

## Design Goals

- Keep approval handling simple and auditable.
- Use one payload shape for all delivery paths.
- Avoid process-lifetime races from short-lived CLI commands.
- Ensure chat/session continuation can happen automatically.
- Keep pending notifications durable until acknowledged.

## Structured Payload Contract

Approval resolution uses a structured message body:

```json
{
  "type": "approval_resolved",
  "request_id": "apr-1234abcd",
  "agent_id": "weather.fetcher",
  "status": "approved",
  "install_completed": true,
  "message": "Agent approved and installed."
}
```

The same schema is used for approval notifications regardless of delivery path.

## Storage Model

- Pending approvals: `.gateway/scheduler/approvals/pending/<request_id>.json`
- Approved decisions: `.gateway/scheduler/approvals/approved/<request_id>.json`
- Rejected decisions: `.gateway/scheduler/approvals/rejected/<request_id>.json`
- Session notifications: `.gateway/signal/<session_id>/<request_id>.json`

Delivery bookkeeping markers can exist beside signal files (for example `.delivered` markers).

## Delivery Ownership

- `gateway approvals approve|reject` records approval decisions.
- Notification polling and delivery are started by the gateway daemon (`gateway start`), not by the short-lived approval CLI process.
- Chat/TUI acts as a channel consumer and can resume on its own connection.

## Chat Channel Behavior

For terminal chat:

1. chat detects pending approval signal files for its `session_id`,
2. chat emits an `event.ingest` resume on its own socket with the structured approval payload,
3. chat renders the assistant continuation,
4. chat consumes the signal only after successful resume/render.

If resume fails, the signal remains pending for retry.

## Background Scheduler Behavior

Background wake on approval is gated by `background.wake_predicates.approval_resolved`.

If disabled, approved requests do not trigger `WakeReason::ApprovalResolved`.

## Operator Expectations

- Approving a request should queue a durable notification for the waiting session.
- If chat for that session is already open, continuation should appear without manual `continue`/`done` prompts.
- If no consumer is connected yet, the notification stays pending until a consumer acknowledges it.

## Current Caveats

- Some target routing for parent/child sessions still relies on existing session conventions.
- Signal persistence and delivery markers are durable, but consumer-side acknowledgement remains the primary completion signal.

## Related Docs

- `docs/quickstart-planner-specialist-chat.md`
- `docs/agent_routing_and_roles.md`
