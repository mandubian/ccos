# Chat Mode Policy Pack (Phase 0)

## Purpose
Define the minimal, default-deny policy pack for chat mode. This pack is designed to prevent raw personal data exposure by default and to require explicit approvals for any new channels, effects, or egress.

## Policy Pack Structure
- **Constitution layer**: high-level, immutable safety rules.
- **Static safety rules**: concrete allow/deny rules enforced at the gateway boundary and GK checkpoints.
- **Activation rules**: per-channel activation and sender allowlist requirements.

## Default-Deny Rules (Required)
### Data Access
- Deny direct access to `pii.chat.message`, `pii.attachment`, and `secret.*` values for all non-transform capabilities.
- Allow access to `pii.chat.metadata` only for routing and session management.

### Network Egress
- Deny network egress for any value classified as `pii.*` or `secret.*`.
- Allow egress only for `public` values by default.
- `pii.redacted` is still **deny-by-default** and only allowed with an explicit policy exception (and corresponding approval/audit trail).

### Filesystem Writes
- Deny filesystem writes of `pii.*` and `secret.*` by default.
- Allowlist-only paths with explicit approval for any writes.

### External Agent Exposure
- Deny exposure of `pii.*` and `secret.*` to external agents by default.

## Transform Capability Rules
- Transform capabilities must declare:
  - Accepted input classifications
  - Output classification and schema
  - Any intended downgrades (e.g., `pii.*` to `pii.redacted`)
- Any downgrade to `public` requires explicit approval.

## Activation & Allowlist Rules
- Group chats require explicit activation by mention/keyword.
- Sender allowlists are required for group channels by default.
- Channel connections require explicit approval before any message processing.

## Approval Triggers (Required)
- New channel connection
- Capability enabling or update that adds effects or expands scope
- Secret insertion or rotation
- Policy exceptions for egress or storage

## Audit Requirements
- Policy decision recorded at every gate:
  - capability execution
  - quarantine access
  - egress attempt
- Denials include explicit reason and policy rule reference.

## Success Criteria
- A connector can receive messages but cannot access or export raw message content without approvals and transform-only access.

## Alignment
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Governance & context: `docs/ccos/specs/005-security-and-context.md`
- Two-tier governance: `docs/ccos/specs/035-two-tier-governance.md`
