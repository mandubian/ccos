# Quarantine Store Contract (Phase 0)

## Purpose
Define the minimal storage contract for quarantined chat content and attachments, ensuring raw personal data is never exposed directly to planning or general tool calls.

## Core Requirements
- Store raw message bodies and attachments as opaque blobs.
- Access is only through approved transform capabilities.
- No direct reads by planning or non-transform tools.
- Every access is audited with policy decision and justification.

## Data Model (Logical)
- **Blob**: opaque bytes, immutable once stored.
- **Pointer**: reference object used in session steps.
- **Metadata**: minimal routing info (timestamps, sender IDs, channel/thread IDs), classified as `pii.chat.metadata`.

## Access Rules
- Only transform capabilities can request blob access.
- Access requires:
  - capability identity
  - justification string
  - declared input classification
- Output must be schema-validated and classified.

## Security Invariants
- **Pointer unguessability**: pointers are random, non-sequential identifiers.
- **Pointer is not authorization**: access is mediated by gateway + GK policy checks.
- **Encryption at rest**: out of scope for Phase 0, required by Phase 1.
- **Size/side-channel policy**: blob sizes may be recorded internally but must not be exposed externally by default.

## Retention Rules (Phase 0 Minimal)
- Default TTL configurable per channel.
- Manual purge supported per session or per sender.
- Purge events recorded in causal chain.

## Audit Events
Each access must record:
- capability ID and version
- pointer ID
- policy decision (allow/deny)
- justification
- output classification
- run/session/step identifiers
- policy pack version and rule ID

## Security Guarantees
- Gateway enforces quarantine boundary before GK execution.
- No “raw export” capability is permitted without explicit approval and policy exception.

## Alignment
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Capability system architecture: `docs/ccos/specs/030-capability-system-architecture.md`
- Sandbox isolation: `docs/ccos/specs/022-sandbox-isolation.md`
