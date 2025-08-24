# CCOS Specification: Intent Event Sink

## Overview

The IntentEventSink is the mandatory audit surface for intent lifecycle status transitions. It centralizes emission of audited status change events and ensures failures are surfaced (no best-effort logging).

## Contract

### Core trait (conceptual signature)

```rust
pub trait IntentEventSink: Debug {
    /// Emit an intent status change event for mandatory audit
    fn log_intent_status_change(
        &self,
        plan_id: &str,
        intent_id: &IntentId,
        old_status: &str,
        new_status: &str,
        reason: &str,
        triggering_action_id: Option<&str>,
    ) -> Result<(), RuntimeError>;
}
```

### Behavioral guarantees
- Implementations must ensure either:
  - The status change is durably recorded (preferred), or
  - An explicit error is returned and propagated to the caller so the transition can fail/retry.
- No best-effort logging: lifecycle transitions call the sink and treat failures as runtime errors.
- When lifecycle completes an intent, the resulting status depends on execution result: success → Completed, failure → Failed (and a corresponding audit action is emitted).

## Implementations
- NoopIntentEventSink
  - Purpose: compatibility for unit tests that intentionally don't assert audit trails.
  - Behavior: always returns Ok(()) and does not mutate external state.
- CausalChainIntentEventSink
  - Purpose: production sink appending an IntentStatusChanged action into the CausalChain.
  - Behavior: locks the CausalChain (internal Arc<Mutex<CausalChain>>) and appends an audited action.
  - Failure modes: locking failure or ledger append failure map to RuntimeError and are returned upstream.

## Integration points
- IntentGraph owns an Arc<dyn IntentEventSink> constructed at system init (CCOS::new) and passes it through lifecycle mutations.
- All lifecycle mutation paths (orchestrator, lifecycle manager, bulk ops, archival) must use the sink and never bypass it.

## Error handling and recovery
- On sink error, lifecycle methods return an error and do not silently drop the audit event.
- Caller (orchestrator / CCOS) may decide whether to retry the transition after handling the error.

## Audit data format
- IntentStatusChanged action metadata includes:
  - plan_id (optional)
  - intent_id
  - old_status
  - new_status
  - reason
  - triggering_action_id (optional)
  - signature (added by causal-chain signing)

## Testing guidance
- Unit tests may use NoopIntentEventSink when audit assertions are not required.
- Integration tests should use CausalChainIntentEventSink with an in-memory CausalChain and assert presence of IntentStatusChanged actions via get_actions_for_intent.

## Migration notes
- Previous optional causal chain parameter was removed. Callers must be constructed with an event sink.
- If a new sink implementation performs network/blocking I/O, prefer asynchronous design and ensure non-blocking call paths or background tasks.

## Security and privacy
- The sink may record sensitive metadata (intent goals or reasons). Implementations must honor runtime privacy/security policies (e.g., redact secrets).

## Revision history
- 2025-08-19: Initial specification and integration tests added.
