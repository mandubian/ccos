# Chat Mode Approval Flow (Phase 0)

## Purpose
Define the minimal approval flow required to enable channels, capabilities, secrets, and policy exceptions in chat mode.

## Approval Types
- **Channel connection approval**
- **Capability enable/upgrade approval**
- **Secret insertion/rotation approval**
- **Policy exception approval** (egress or storage)
- **Public declassification approval** (downgrade `pii.*` → `public`)

## Required Steps
1. **Request creation**
   - Includes requester, scope, justification, and proposed changes.
2. **Policy diff generation**
   - List new effects/permissions and their impact.
3. **Human decision**
   - Approve / deny / modify scope.
4. **Audit log**
   - Decision recorded to causal chain.

## Minimal CLI Flow (MVP)
- `list-approvals`
- `show-approval <id>`
- `approve <id>`
- `deny <id>`
- `modify <id> --scope ...`

## Enforcement
- No side effects occur until approval is granted.
- Approval is required for any downgrade to `public` of `pii.*` values.
- **Verifier requirement (hard rule)**: approval alone is not sufficient to release `public` output.
  - Any `pii.*` → `public` release MUST be gated by the **Redaction Verifier** capability and must pass (`ok=true`) before egress is allowed.
  - Approval grants permission to *attempt* the downgrade under declared constraints; the verifier decides whether the specific output may be released.

## Public Declassification Approval (Required Wiring)
This is the concrete wiring that makes the “verifier pass required” rule enforceable.

### 1) Approval Request Contents (minimum)
When requesting permission to downgrade to `public`, the request MUST include:
- **input classification** (must be `pii.*`)
- **intended output classification** (`public`)
- **transform capability ID/version** producing the candidate output (see `045-chat-transform-capabilities.md`)
- **verifier capability ID/version** that will validate the output
- **constraints** to be enforced by the verifier (e.g., no verbatim quotes, identifiers removed, max length)
- **scope**: which channel/session/run the approval applies to (per-run by default)

### 2) Enforcement Sequence
1. Transform runs and produces `pii.redacted` output.
2. Approval is requested (or checked) for `pii.redacted` → `public` downgrade.
3. If approved, the system runs the **Redaction Verifier** on the candidate output.
4. Only if verifier returns `ok=true` may the output be treated as `public` for egress.
5. If verifier returns `ok=false`, the downgrade is denied (no egress), and issues are recorded.

### 3) Audit Requirements
The causal chain MUST record:
- the approval decision and scope
- verifier invocation and result (`ok`, `issues`)
- the policy rule ID that allowed/denied the final egress attempt
- Public downgrades require **per-run approval** and a successful **redaction verifier** pass before egress.

## Alignment
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Two-tier governance: `docs/ccos/specs/035-two-tier-governance.md`
- Chat transform capabilities (verifier): `docs/ccos/specs/045-chat-transform-capabilities.md`