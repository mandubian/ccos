# Hello World Connector Flow (Phase 0/1)

## Purpose
Define a minimal, end-to-end connector flow that demonstrates safe message intake, quarantine storage, transform-only access, and governed outbound replies.

## Preconditions
- Chat mode policy pack is active.
- Quarantine store is configured.
- Activation rules and allowlists are configured for the channel.

## Flow Overview
1. **Inbound message arrives** at connector.
2. Connector stores raw body in **quarantine** and receives `content_ref`.
3. Connector emits a `MessageEnvelope` containing metadata + `content_ref`.
4. Gateway records a session step with metadata only.
5. Planning requests a **transform capability** (e.g., summary).
6. Transform reads quarantine, returns schema-validated `pii.redacted` output.
7. Policy check evaluates egress:
   - Only `public` may egress by default.
   - `pii.redacted` egress is **deny-by-default** and requires an explicit policy exception + approval/audit trail.
   - Any `pii.* → public` downgrade requires **per-run approval** and a successful **Redaction Verifier** pass before egress.
8. Outbound reply sent via connector and audited.

## Sequence (Logical)
- `connector.on_message(raw)`
  - `quarantine.put(raw.body)` → `content_ref`
  - `emit(MessageEnvelope)`
- `gateway.record_step(envelope)`
- `planner.request(transform.summarize, content_ref)`
- `transform.read_quarantine(content_ref)`
- `transform.return(output, classification=pii.redacted)`
- `policy.check(egress, output)`
  - if attempting `pii.redacted` egress: require explicit exception + approval
  - if attempting `public` downgrade: require per-run approval + `transform.verify_redaction(output)` → `{ok, issues}`
- `connector.send(outbound)`

## Acceptance Criteria
- Raw content never reaches planning or outbound send without transform.
- All quarantine reads and egress decisions are audited.
- A reply can be sent using `public` output by default.
- `pii.redacted` replies require an explicit policy exception (deny-by-default) and must be audited.
- Any downgrade to `public` is verifier-gated and audited.

## Alignment
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Quarantine store contract: `docs/ccos/specs/039-quarantine-store-contract.md`
- Normalized message schema: `docs/ccos/specs/042-chat-message-schema.md`
- Connector adapter interface: `docs/ccos/specs/043-chat-connector-adapter.md`
