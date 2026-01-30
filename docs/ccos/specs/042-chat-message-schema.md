# Normalized Chat Message Schema (Phase 0/1)

## Purpose
Define a single, normalized schema for inbound and outbound chat events across connectors. This schema is designed to carry **metadata only by default** and to reference quarantined content via pointers.

## Core Principles
- Raw message bodies and attachments are stored in quarantine.
- The normalized schema holds only metadata and pointers.
- Classification is applied to every field.

## Schema (Logical)
### MessageEnvelope
- `id`: unique message ID (string) → `pii.chat.metadata`
- `channel_id`: connector-local channel/thread ID (string) → `pii.chat.metadata`
- `sender_id`: connector-local sender ID (string) → `pii.chat.metadata`
- `timestamp`: RFC3339 timestamp (string) → `pii.chat.metadata`
- `direction`: `inbound | outbound` → `internal.system`
- `content_ref`: pointer to quarantined message body (opaque) → `pii.chat.message`
- `attachments`: list of `AttachmentRef` → `pii.attachment`
- `thread_id`: optional thread identifier (string) → `pii.chat.metadata`
- `reply_to`: optional message ID (string) → `pii.chat.metadata`
- `activation`: optional activation metadata (mention/keyword match) → `internal.system`
- `labels`: derived classification tags attached by the gateway (optional, internal only) → `internal.system`

### AttachmentRef
- `id`: unique attachment ID (string) → `pii.chat.metadata`
- `content_ref`: pointer to quarantined attachment blob → `pii.attachment`
- `content_type`: MIME type (string) → `pii.chat.metadata`
- `size_bytes`: integer → `pii.chat.metadata`
- `filename`: optional string → `pii.chat.metadata`

## Outbound Messages
Outbound messages must reference **public** or approved `pii.redacted` content only. If a reply requires raw content, it must be produced by an approved transform capability and explicitly approved for egress.

## Audit Requirements
- Every envelope creation is logged with classifications.
- Any dereference of `content_ref` is logged as a quarantine access event.

## Alignment
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Quarantine store contract: `docs/ccos/specs/039-quarantine-store-contract.md`
