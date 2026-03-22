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

Autonoetic now uses an embedded SQLite database (`GatewayStore`) for durable approval and notification storage, replacing the legacy file-based system.

- **Database:** `.gateway/gateway.db`
- **Approvals:** Stored in the `approvals` table with columns for `request_id`, `agent_id`, `session_id`, `status` (`pending`, `approved`, `rejected`), etc.
- **Notifications:** Stored in the `notifications` table (replacing `.gateway/signal/` files), tracking delivery attempts and consumption status.
- **Workflow Events:** Stored in the `workflow_events` table for resilient event logging.

Delivery bookkeeping (like attempt counts and `consumed_at` timestamps) is natively tracked as columns within the SQLite schema.

## Delivery Ownership

- `gateway approvals approve|reject` records approval decisions.
- Notification polling and delivery are started by the gateway daemon (`gateway start`), not by the short-lived approval CLI process.
- Chat/TUI acts as a channel consumer and can resume on its own connection.

## Chat Channel Behavior

For terminal chat:

1. chat observes the causal chain for `approval_resolved` workflow events or background continuation,
2. chat renders the assistant continuation automatically based on the updated causal events.

Signal files are no longer polled or consumed by the CLI; this is now entirely managed by the Gateway's internal SQLite notification system.

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
