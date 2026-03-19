# Approval Response Details Plan

## Goal

Improve approval-blocked tool responses so agents and operators can see exactly what needs approval without reverse-engineering a generic message.

Today the gateway mostly returns:

- `approval_required: true`
- `request_id`
- a human string

That is enough to continue the flow, but not enough to:

- explain the approval subject clearly to the user
- let agents summarize the approval request reliably
- show operators the important risk details at the point of refusal

The gateway already has richer structured data in `ApprovalRequest` and `ScheduledAction`.
This plan makes the runtime responses expose a compact, safe subset of that structure.

## Scope

Primary targets:

- `sandbox.exec` approval-required response
- `agent.install` approval-required response

Likely files:

- `autonoetic-gateway/src/runtime/tools.rs`
- `autonoetic-types/src/background.rs`
- tests in `autonoetic-gateway/src/runtime/tools.rs`
- `autonoetic-gateway/tests/agent_install_approval_e2e.rs`

Optional follow-up:

- CLI rendering improvements in `autonoetic/src/cli/chat.rs`
- approval listing / display improvements in gateway CLI

## Design Principles

1. Return structured approval details, not just prose
2. Reuse the existing `ApprovalRequest.action` model where possible
3. Keep the response compact and safe
4. Include enough detail for human review and agent explanation
5. Do not dump full payloads or large code blobs into normal tool responses

## Target Response Shape

Approval-blocked responses should keep existing fields for compatibility:

- `ok`
- `approval_required`
- `request_id`
- `message`

And add a new structured object:

- `approval`

Suggested shape:

```json
{
  "ok": false,
  "approval_required": true,
  "request_id": "apr-1234abcd",
  "message": "Install requires approval...",
  "approval": {
    "kind": "agent_install",
    "reason": "High-risk install requires approval",
    "summary": "Weather fetcher with NetworkAccess",
    "requested_by_agent_id": "specialized_builder.default",
    "session_id": "demo-session/planner-123",
    "retry_field": "promotion_gate.install_approval_ref",
    "subject": {
      "...": "tool-specific fields"
    }
  }
}
```

## Tool-Specific Details

## 1. `sandbox.exec`

Return:

- `kind: "sandbox_exec"`
- `reason`
- `summary`
- `retry_field: "approval_ref"`
- `subject.command`
- `subject.dependencies`
- `subject.remote_access_detected`
- `subject.detected_patterns`

If cheaply derivable from current analysis, also include:

- `subject.normalized_targets`
- `subject.hosts`

Do not include:

- full script content
- full mounted session content
- excessive raw analysis dumps

## 2. `agent.install`

Return:

- `kind: "agent_install"`
- `reason`
- `summary`
- `requested_by_agent_id`
- `session_id`
- `retry_field: "promotion_gate.install_approval_ref"`
- `subject.agent_id`
- `subject.artifact_id`
- `subject.capabilities`
- `subject.background_enabled`
- `subject.scheduled_action_kind`
- `subject.execution_mode`
- `subject.script_entry`

If available, also include:

- `subject.risk_factors`
- `subject.artifact_digest`

Risk factors should be summarized, not inferred by the caller from raw capability lists.
Examples:

- `network_access`
- `code_execution`
- `broad_write_access`
- `background_execution`
- `scheduled_action`

## Implementation Phases

## Phase 1: Response Schema

Tasks:

- [x] add an `approval` object to approval-blocked tool responses
- [x] keep existing top-level fields for backward compatibility
- [x] standardize common fields:
  - `kind`
  - `reason`
  - `summary`
  - `requested_by_agent_id`
  - `session_id`
  - `retry_field`
  - `subject`

## Phase 2: `sandbox.exec`

Tasks:

- [x] add structured approval details to the remote-access approval response
- [x] expose detected remote-access summary in machine-friendly form
- [x] include dependency summary when present
- [x] keep `detected_patterns` at either top level or inside `approval.subject`, but avoid duplicate confusion

## Phase 3: `agent.install`

Tasks:

- [x] add structured approval details to the install approval response
- [x] compute and expose `risk_factors`
- [x] include `artifact_id`
- [x] include install shape details relevant to operator review
- [x] ensure the structured response matches the actual `ApprovalRequest` written to disk

## Phase 4: Shared Helper

Tasks:

- [x] add a helper to build approval response payloads from `ApprovalRequest` + local tool context
- [x] avoid hand-building slightly different JSON shapes in each tool
- [x] keep the helper role-agnostic and action-oriented

This helper should reduce drift between:

- the runtime response returned to the agent
- the pending approval JSON written by the gateway

## Phase 5: Tests

Tasks:

- [x] add unit tests for `sandbox.exec` approval response shape
- [x] add unit tests for `agent.install` approval response shape
- [x] assert presence of:
  - `approval.kind`
  - `approval.reason`
  - `approval.summary`
  - `approval.retry_field`
  - tool-specific `approval.subject` fields
- [x] verify old fields like `approval_required` and `request_id` still exist

## Phase 6: Optional UX Follow-Up

Tasks:

- [ ] update CLI/TUI rendering to display structured approval summaries cleanly
- [ ] use `approval.kind` and `approval.subject` instead of parsing free text
- [ ] show a concise operator review card when available

## Open Decisions

1. Should the new `approval` object mirror `ApprovalRequest` exactly?

Recommended:

- no
- keep it close, but optimize for runtime UX and compactness

2. Should `detected_patterns` remain top-level in `sandbox.exec` responses?

Recommended:

- keep it for now for compatibility
- also include it under `approval.subject` or move later in a cleanup pass

3. Should we expose raw capability lists and separate `risk_factors`?

Recommended:

- yes
- raw declared capabilities are useful
- `risk_factors` are the operator-facing interpretation

4. Should approval details include full artifact manifest contents?

Recommended:

- no
- include `artifact_id`, digest, and a compact summary only
- deeper inspection belongs in explicit approval inspection tooling

## Acceptance Criteria

- approval-blocked responses include a structured `approval` object
- `sandbox.exec` responses explain what remote-capable action is awaiting approval
- `agent.install` responses explain what install is awaiting approval and why
- agents can explain approval requirements without parsing prose
- existing retry flow remains compatible
- response details stay compact and do not expose unnecessary payload data

## Short Version

The gateway should not only say "approval required."
It should say:

- what action needs approval
- why it needs approval
- what exactly is being approved
- how to retry after approval

in a compact structured form that agents and UIs can use directly.
