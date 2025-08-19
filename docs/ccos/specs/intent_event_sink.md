<<<<<<< HEAD
# CCOS Specification: Intent Event Sink

## Overview

The `IntentEventSink` is an abstraction for mandatory intent lifecycle audit in CCOS. It provides a centralized mechanism for emitting intent status change events that must be recorded for auditability and compliance.

## Contract

### Core Trait

```rust
pub trait IntentEventSink: Send + Sync + Debug {
    /// Emit an intent status change event for mandatory audit
    fn emit_status_change(
        &self,
        intent_id: &IntentId,
    new_status,
        # IntentEventSink (Audit) Specification

        Purpose
        - Define the `IntentEventSink` abstraction: mandatory audit surface for intent lifecycle status transitions.
        - Document usage, guarantees, and expectations for implementations (CausalChain-backed, noop for legacy tests only).

        Contract
        - Trait signature (conceptual)
            - fn log_intent_status_change(
            - &self,
            - plan_id: &str,
            - intent_id: &IntentId,
            - old_status: &str,
            - new_status: &str,
            - reason: &str,
            - triggering_action_id: Option<&str>
            - ) -> Result<(), RuntimeError>;

        Behavioral guarantees
        - Implementations must ensure either:
            - The status change is durably recorded (preferred), or
            - An explicit error is returned and propagated to the caller so the transition can fail/retry.
        - No best-effort logging: lifecycle transitions call the sink and treat failures as runtime errors.

        Implementations
        - NoopIntentEventSink
            - Purpose: compatibility for unit tests that intentionally don't assert audit trails.
            - Behavior: always returns Ok(()) and does not mutate external state.
        - CausalChainIntentEventSink
            - Purpose: production sink that appends an `IntentStatusChanged` Action into the `CausalChain` ledger.
            - Behavior: locks the `CausalChain` (internal Arc<Mutex<CausalChain>>) and calls `log_intent_status_change` on it.
            - Failure modes: locking failure or ledger append failure map to `RuntimeError` and are returned upstream.

        Integration points
        - `IntentGraph` holds an `Arc<dyn IntentEventSink>` constructed at system init (`CCOS::new`) and passed to lifecycle methods.
        - All lifecycle mutation paths (orchestrator, lifecycle manager, bulk operations, archival) accept or use the sink and never bypass it.

        Error handling and recovery
        - On sink error, the lifecycle method must return an error and not silently drop the audit event.
        - Caller (orchestrator / CCOS) may decide whether to retry the transition after handling the error.

        Audit data format
        - `IntentStatusChanged` action metadata includes:
            - `plan_id`: optional plan id if available
            - `intent_id`
            - `old_status`
            - `new_status`
            - `reason`
            - `triggering_action_id` (optional)
            - `signature` (added by causal chain signing)

        Testing guidance
        - Unit tests may use `NoopIntentEventSink` when audit assertions are not required.
        - Integration tests should use `CausalChainIntentEventSink` with an in-memory `CausalChain` and assert that `get_actions_for_intent` contains `IntentStatusChanged` actions.

        Migration notes
        - Previous optional causal chain parameter was removed. Callers must provide or be constructed with an event sink.
        - If a new sink implementation is added that performs network or blocking I/O, prefer making it asynchronous and ensure callers call it inside non-blocking contexts or use background tasks.

        Security and privacy
        - The sink may record sensitive metadata (intent goals or reasons). Implementations must honor the runtime's privacy/security policies when adding metadata (e.g., redact secrets).

        Revision history
        - 2025-08-19: Initial specification and integration tests added.
        ``` 
- No best-effort logging: lifecycle transitions call the sink and treat failures as runtime errors.
- Note: when the lifecycle manager marks an intent as "complete", the resulting stored intent status depends on the execution outcome: if `ExecutionResult.success` is false, the lifecycle will transition the intent to `Failed` (and emit a `Failed` audit action) rather than `Completed`.

Implementations
- NoopIntentEventSink
  - Purpose: compatibility for unit tests that intentionally don't assert audit trails.
  - Behavior: always returns Ok(()) and does not mutate external state.
- CausalChainIntentEventSink
  - Purpose: production sink that appends an `IntentStatusChanged` Action into the `CausalChain` ledger.
  - Behavior: locks the `CausalChain` (internal Arc<Mutex<CausalChain>>) and calls `log_intent_status_change` on it.
  - Failure modes: locking failure or ledger append failure map to `RuntimeError` and are returned upstream.

Integration points
- `IntentGraph` holds an `Arc<dyn IntentEventSink>` constructed at system init (`CCOS::new`) and passed to lifecycle methods.
- All lifecycle mutation paths (orchestrator, lifecycle manager, bulk operations, archival) accept or use the sink and never bypass it.

Error handling and recovery
- On sink error, the lifecycle method must return an error and not silently drop the audit event.
- Caller (orchestrator / CCOS) may decide whether to retry the transition after handling the error.

Audit data format
- `IntentStatusChanged` action metadata includes:
  - `plan_id`: optional plan id if available
  - `intent_id`
  - `old_status`
  - `new_status`
  - `reason`
  - `triggering_action_id` (optional)
  - `signature` (added by causal chain signing)

Testing guidance
- Unit tests may use `NoopIntentEventSink` when audit assertions are not required.
- Integration tests should use `CausalChainIntentEventSink` with an in-memory `CausalChain` and assert that `get_actions_for_intent` contains `IntentStatusChanged` actions.

Migration notes
- Previous optional causal chain parameter was removed. Callers must provide or be constructed with an event sink.
- If a new sink implementation is added that performs network or blocking I/O, prefer making it asynchronous and ensure callers call it inside non-blocking contexts or use background tasks.

Security and privacy
- The sink may record sensitive metadata (intent goals or reasons). Implementations must honor the runtime's privacy/security policies when adding metadata (e.g., redact secrets).

Revision history
- 2025-08-19: Initial specification and integration tests added.
>>>>>>> f88c5e1 (tests: add lifecycle audit tests; docs: intent_event_sink spec)
