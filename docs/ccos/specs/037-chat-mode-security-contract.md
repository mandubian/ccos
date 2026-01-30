# Chat Mode Security Contract (Phase 0)

## Purpose
Define the minimum security and data-protection contract required to operate a chat gateway without exposing raw personal data by default. This contract applies to **values** flowing through CCOS and is enforced at the gateway/adapter boundary and the governance kernel (GK).

## Scope
- Inbound chat messages and attachments
- Gateway session state and routing metadata
- Transform capabilities that access quarantined data
- Data egress to network, filesystem, or external agents

## Definitions
### Personally Identifiable Information (PII)
PII is any data that can identify, contact, or uniquely single out a person, directly or indirectly. In chat contexts, **PII includes but is not limited to**:
- Message bodies, quotes, and paraphrases that preserve identifiable content
- Attachments and derived content (images, audio, transcripts)
- Identifiers: names, handles, email, phone, address, government IDs, account IDs
- Device/user identifiers: IP addresses, device IDs, cookies, session tokens
- Sensitive metadata that can re-identify a person when combined with other data

If classification is uncertain, treat the value as PII.

### Classification Labels (Required)
Labels apply to **values**, not only capabilities:
- `public`: safe to expose and export
- `pii.chat.message`: raw message text or equivalent
- `pii.chat.metadata`: sender IDs, channel IDs, timestamps, thread IDs
- `pii.attachment`: raw attachment content or derived text
- `pii.redacted`: redacted/summary output that may still be sensitive
- `secret.token`: credentials, API keys, session tokens
- `internal.system`: system-only operational data

## Classification Rules
1. **Default classification** for inbound data:
   - Message text → `pii.chat.message`
   - Attachments → `pii.attachment`
   - Sender/channel/thread metadata → `pii.chat.metadata`
2. **Propagation**: derived values inherit the most restrictive classification among inputs.
3. **Declassification** is only allowed by **approved transform capabilities** and is exceptional:
  - Default transform output class is `pii.redacted`.
  - Downgrade to `public` requires **per-run approval** and output constraints (no verbatim quotes, identifiers removed, max length enforced).
  - A redaction verifier pass is required before `public` outputs are released.
4. **Unknowns** default to PII.

## Classification Lattice
### Partial Order (least restrictive → most restrictive)
`public` < `pii.redacted` < `pii.chat.message`
`public` < `pii.redacted` < `pii.attachment`
`public` < `pii.redacted` < `pii.chat.metadata`
`secret.token` is **incomparable** with `pii.*` but **dominates** all egress decisions (treated as most restrictive for export).

### Join Operation (Propagation)
`join(a, b)` = the most restrictive classification in the lattice.
- If either side is `secret.token`, the result is `secret.token`.
- Otherwise pick the maximum restriction by the ordering above.

### Metadata Egress
`pii.chat.metadata` is **not allowed** to egress by default; it is only permitted inside gateway/runtime for routing and session management.

## Enforcement Points (Operational)
- **Classification attachment**: labels are attached to RTFS `Value` at runtime.
- **Mixed outputs**: per-field labels are supported for maps/objects; any unlabeled field defaults to the most restrictive label among siblings.
- **MCP boundary**: every tool result leaving the gateway/MCP server is filtered by policy; `pii.*` and `secret.*` are denied unless explicitly approved.

### Example: Per-Field Labeling
```
{
  "summary": { "value": "Meeting recap", "label": "pii.redacted" },
  "tasks":   { "value": ["Send draft"], "label": "pii.redacted" },
  "public":  { "value": "OK to share", "label": "public" }
}
```
Any missing labels are treated as the most restrictive label present in the object.

## Quarantine Store Contract
- Raw message bodies and attachments are stored as opaque blobs in a quarantined store.
- Only transform capabilities can access quarantine.
- Quarantine access is **never** exposed directly to general planning or tool calls.
- Each access is logged with justification, policy decision, and capability identity.

## Transform Capabilities (Required)
Transform capabilities are narrow, schema-checked operations that can read quarantine data and return limited outputs:
- Examples: **summarize**, **extract entities**, **redact**, **classify**
- Each transform declares:
  - **Input classification(s)** (e.g., `pii.chat.message`)
  - **Output classification** (e.g., `pii.redacted` or `public`)
  - **Schema** for output validation
- Transforms must not return raw content by default.
- Any downgrade to `public` requires explicit approval and audit logging.

## Egress Defaults (Hard Rules)
- **Network egress** of `pii.*` and `secret.*` is denied by default.
- **Filesystem writes** of `pii.*` and `secret.*` are denied by default, unless allowlisted path + approval is present.
- **External agent exposure** of `pii.*` and `secret.*` is denied by default.

## Activation & Consent (Chat Safety)
- Group chats require explicit activation (mention/keyword) and sender allowlists.
- Activation rules are part of policy configuration and audited per session.

## Audit Requirements
Every chat gateway run must record:
- Inbound message metadata (no raw content by default)
- Classification decisions and any overrides
- Quarantine access events with justification
- Policy decisions for egress, approvals, and denials

## Success Criteria (Phase 0)
- A basic connector can receive messages but **cannot** expose raw content without an explicit, audited transform capability.
- All PII egress remains denied by default.

## Alignment
- Host boundary: `docs/ccos/specs/004-rtfs-ccos-boundary.md`
- Governance & context: `docs/ccos/specs/005-security-and-context.md`
- Two-tier governance: `docs/ccos/specs/035-two-tier-governance.md`
- Sandbox isolation: `docs/ccos/specs/022-sandbox-isolation.md`
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Quarantine store contract: `docs/ccos/specs/039-quarantine-store-contract.md`
- Normalized message schema: `docs/ccos/specs/042-chat-message-schema.md`
- Connector adapter interface: `docs/ccos/specs/043-chat-connector-adapter.md`
- Chat transform capabilities: `docs/ccos/specs/045-chat-transform-capabilities.md`
- Chat approval flow: `docs/ccos/specs/046-chat-approval-flow.md`
- Chat audit events: `docs/ccos/specs/047-chat-audit-events.md`
