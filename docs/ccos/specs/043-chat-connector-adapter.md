# Chat Connector Adapter Interface (Phase 0/1)

## Purpose
Define the minimal adapter interface for chat connectors so Phase 1 can implement a single connector against a stable contract.

## Responsibilities
- Normalize inbound messages into `MessageEnvelope` with quarantine pointers.
- Enforce activation rules (mentions/keywords + allowlists).
- Enforce loopback-only control plane defaults.
- Emit outbound messages only after policy approval and classification checks.
- Never materialize raw content into logs or error messages.

## Adapter Interface (Logical)
### Required Functions
- `connect(config) -> ConnectionHandle`
- `disconnect(handle)`
- `subscribe(handle, callback)`
  - Callback receives `MessageEnvelope`
- `send(handle, outbound_request) -> SendResult`
- `health(handle) -> HealthStatus`

### Outbound Request
- `channel_id`
- `content` (must be `public` or approved `pii.redacted`)
- `reply_to` (optional)
- `metadata` (optional)

## Gateway Enforcement Points
- Before `subscribe`: verify channel approval + activation rules.
- Before `send`: verify policy pack and classification for payload.
- Every send is audited with policy decision and classification details.

## Error Handling
- Connection errors must be categorized (auth, network, rate-limit).
- Retries are governed by run budget and policy.

## Alignment
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Normalized message schema: `docs/ccos/specs/042-chat-message-schema.md`
- Hello world connector flow: `docs/ccos/specs/044-hello-world-connector-flow.md`
