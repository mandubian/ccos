# Chat Transform Capabilities (Phase 0)

## Purpose
Define the minimal set of transform capabilities permitted to access quarantine data and return schema-validated outputs for safe planning and replies.

## Required Capabilities
### 1) Summarize Message
- **Input**: `pii.chat.message`
- **Output**: `pii.redacted`
- **Schema**: `{ summary: string, topics?: string[], tasks?: string[] }`
- **Notes**: Must not return raw quotes unless explicitly approved.

### 2) Extract Entities/Tasks
- **Input**: `pii.chat.message`
- **Output**: `pii.redacted`
- **Schema**: `{ entities: [{ type: string, value: string }], tasks: string[] }`
- **Notes**: Redact or mask identifiers unless approved for exposure.

### 3) Redact Message
- **Input**: `pii.chat.message`
- **Output**: `pii.redacted` or `public` (approval required for `public`)
- **Schema**: `{ redacted_text: string, redactions: [{ span: [number, number], type: string }] }`
- **Notes**: `public` output requires explicit policy exception.

### 5) Redaction Verifier (Required for `public`)
- **Input**: `pii.redacted`
- **Output**: `public` (only if verifier passes)
- **Schema**: `{ ok: boolean, issues?: string[] }`
- **Notes**: Must enforce constraints (no verbatim quotes, identifiers removed, max length) before any downgrade to `public`.

### 4) Attachment Text Extraction (Optional)
- **Input**: `pii.attachment`
- **Output**: `pii.redacted`
- **Schema**: `{ text: string, mime_type: string }`
- **Notes**: Must run in sandbox and be gated by policy.

## Capability Declarations (Required)
Each transform capability must declare:
- Input classifications
- Output classification
- Output schema
- Any downgrade intent (e.g., `pii.redacted` → `public`)

## Governance Rules
- Quarantine access requires transform capability identity + justification.
- Outputs are schema-validated before returning to planner.
- Any downgrade to `public` requires explicit approval and audit.

## Alignment
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Quarantine store contract: `docs/ccos/specs/039-quarantine-store-contract.md`
