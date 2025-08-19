//! Event sink traits for CCOS component observers
//!
//! Provides lightweight notification mechanisms for various CCOS events.
//! This allows components to stay synchronized without modifying core logic.

use crate::ccos::types::{Action, IntentId, IntentStatus};
use crate::runtime::error::RuntimeError;
use std::fmt::Debug;

/// Trait for components that want to observe Causal Chain events
pub trait CausalChainEventSink: Send + Sync + Debug {
    /// Called when a new action is appended to the causal chain
    fn on_action_appended(&self, action: &Action);
}

/// Trait for components that need to emit intent lifecycle events for audit
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

/// No-op implementation that discards all events  
#[derive(Debug)]
pub struct NoopIntentEventSink;

impl IntentEventSink for NoopIntentEventSink {
    fn emit_status_change(
        &self,
        _intent_id: &IntentId,
        _old_status: &IntentStatus,
        _new_status: &IntentStatus,
        _reason: &str,
        _triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        // No-op: discard all events
        Ok(())
    }
}

/// Implementation that writes status changes to the CausalChain ledger
/// This implementation handles thread safety by delegating to a callback
pub struct CausalChainIntentEventSink<F> 
where 
    F: Fn(&IntentId, &IntentStatus, &IntentStatus, &str, Option<&str>) -> Result<(), RuntimeError> + Send + Sync,
{
    log_fn: F,
}

impl<F> std::fmt::Debug for CausalChainIntentEventSink<F>
where 
    F: Fn(&IntentId, &IntentStatus, &IntentStatus, &str, Option<&str>) -> Result<(), RuntimeError> + Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CausalChainIntentEventSink")
            .field("log_fn", &"<closure>")
            .finish()
    }
}

impl<F> CausalChainIntentEventSink<F>
where 
    F: Fn(&IntentId, &IntentStatus, &IntentStatus, &str, Option<&str>) -> Result<(), RuntimeError> + Send + Sync,
{
    pub fn new(log_fn: F) -> Self {
        Self { log_fn }
    }
}

impl<F> IntentEventSink for CausalChainIntentEventSink<F>
where 
    F: Fn(&IntentId, &IntentStatus, &IntentStatus, &str, Option<&str>) -> Result<(), RuntimeError> + Send + Sync,
{
    fn emit_status_change(
        &self,
        intent_id: &IntentId,
        old_status: &IntentStatus,
        new_status: &IntentStatus,
        reason: &str,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        (self.log_fn)(intent_id, old_status, new_status, reason, triggering_plan_id)
    }
}
