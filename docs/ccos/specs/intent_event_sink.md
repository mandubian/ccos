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
        old_status: &IntentStatus,
        new_status: &IntentStatus,
        reason: &str,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError>;
}
```

### Implementations

#### 1. NoopIntentEventSink

A no-operation implementation that discards all events. Useful for testing or scenarios where audit is disabled.

```rust
pub struct NoopIntentEventSink;
```

#### 2. CausalChainIntentEventSink

A production implementation that writes all status changes to the CausalChain ledger for immutable audit trails.

```rust
pub struct CausalChainIntentEventSink<F> 
where 
    F: Fn(&IntentId, &IntentStatus, &IntentStatus, &str, Option<&str>) -> Result<(), RuntimeError> + Send + Sync,
{
    log_fn: F,
}
```

## Guarantees

### Mandatory Audit

- **All intent status transitions MUST be audited**: Every call to `IntentLifecycleManager::transition_intent_status` requires an `IntentEventSink` parameter
- **No optional audit**: The previous optional `CausalChain` parameter has been replaced with the mandatory sink
- **Fail-fast on audit errors**: If the sink returns an error, the status transition fails entirely

### Atomicity

- Status transitions are atomic: either both the storage update and audit succeed, or neither does
- The storage is updated first, followed by the audit emission
- If audit fails, the calling code should handle rollback if needed

### Thread Safety

- All implementations must be `Send + Sync` for use across threads
- The trait is designed for injection via `Arc<dyn IntentEventSink>`

## Integration Points

### IntentGraph

The `IntentGraph` holds an `Arc<dyn IntentEventSink>` and passes it to all lifecycle operations:

```rust
pub struct IntentGraph {
    pub storage: IntentGraphStorage,
    pub virtualization: IntentGraphVirtualization, 
    pub lifecycle: IntentLifecycleManager,
    pub event_sink: Arc<dyn IntentEventSink>,  // NEW
    pub rt: tokio::runtime::Handle,
}
```

### Lifecycle Manager

All lifecycle methods now require an event sink parameter:

```rust
impl IntentLifecycleManager {
    pub async fn transition_intent_status(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn IntentEventSink,  // NEW: mandatory
        intent: &mut StorableIntent,
        new_status: IntentStatus,
        reason: String,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError>
    
    pub async fn complete_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn IntentEventSink,  // NEW: mandatory
        intent_id: &IntentId,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError>
    
    // ... all other lifecycle methods updated similarly
}
```

## Audit Metadata Format

When emitted through a CausalChain-backed sink, events are recorded as `ActionType::IntentStatusChanged` with the following metadata:

```rust
{
    "old_status": "Active",           // Debug format of old status
    "new_status": "Completed",        // Debug format of new status  
    "reason": "Intent completed successfully",  // Human-readable reason
    "transition_timestamp": "1703123456",       // Unix timestamp
    "signature": "0x...",             // Cryptographic signature
    // Additional context may be added by specific implementations
}
```

## Usage Examples

### Creating an IntentGraph with CausalChain Audit

```rust
let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
let chain_clone = causal_chain.clone();

let event_sink = Arc::new(CausalChainIntentEventSink::new(
    move |intent_id, old_status, new_status, reason, triggering_plan_id| {
        let mut chain = chain_clone.lock()?;
        let plan_id = triggering_plan_id.unwrap_or("intent-lifecycle-manager");
        let old_status_str = format!("{:?}", old_status);
        let new_status_str = format!("{:?}", new_status);
        
        chain.log_intent_status_change(
            &plan_id.to_string(),
            intent_id,
            &old_status_str,
            &new_status_str,
            reason,
            None,
        )
    }
));

let intent_graph = IntentGraph::with_event_sink(event_sink)?;
```

### Creating an IntentGraph with No Audit

```rust
let intent_graph = IntentGraph::new()?;  // Uses NoopIntentEventSink by default
```

### Direct Lifecycle Management

```rust
let lifecycle = IntentLifecycleManager;
let event_sink = Arc::new(NoopIntentEventSink);

lifecycle.complete_intent(
    &mut storage, 
    event_sink.as_ref(), 
    &intent_id, 
    &execution_result
).await?;
```

## Migration Guide

### From Optional CausalChain to Mandatory EventSink

**Before:**
```rust
lifecycle.transition_intent_status(
    storage,
    Some(&mut causal_chain),  // Optional
    &mut intent,
    new_status,
    reason,
    plan_id,
).await?;
```

**After:**
```rust  
lifecycle.transition_intent_status(
    storage,
    event_sink.as_ref(),     // Mandatory
    &mut intent,
    new_status,
    reason,
    plan_id,
).await?;
```

### ExecutionResult Semantics

The `complete_intent` method now properly handles failure cases:

- `ExecutionResult { success: true, .. }` → `IntentStatus::Completed`
- `ExecutionResult { success: false, .. }` → `IntentStatus::Failed`

Both transitions are audited through the event sink.

## Testing

Integration tests are provided in `tests/intent_lifecycle_audit_tests.rs` to validate:

1. Basic event sink functionality
2. IntentGraph integration with event sinks
3. Proper failure case handling (success=false → Failed status)
4. Complete lifecycle audit coverage
5. All status transition methods emit events

## Performance Considerations

- Event sinks should be lightweight and avoid blocking operations
- Heavy processing should be deferred by the sink implementation
- The CausalChain-backed sink provides async-safe logging through closures
- Consider using buffering or background threads for high-frequency audit events

## Security Implications

- All intent operations are now mandatorily audited
- Audit failures prevent status transitions, ensuring no untracked changes
- Cryptographic signing is handled by the CausalChain implementation
- The abstraction allows for future security enhancements without API changes