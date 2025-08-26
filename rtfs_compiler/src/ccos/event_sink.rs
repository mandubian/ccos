//! Event sink traits for CCOS component observers
//!
//! Provides lightweight notification mechanisms for various CCOS events and
//! defines the mandatory `IntentEventSink` audit surface used by lifecycle
//! code paths.

use crate::ccos::types::{Action, IntentId};
use crate::runtime::RuntimeError;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use super::causal_chain::CausalChain;
use std::result::Result;

/// Mandatory audit surface for intent lifecycle transitions.
/// The API uses stable string status representations for compatibility across versions.
pub trait IntentEventSink: Debug + 'static {
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

/// No-op implementation used for legacy tests or callers that explicitly opt-out.
#[derive(Debug, Default, Clone)]
pub struct NoopIntentEventSink;

impl IntentEventSink for NoopIntentEventSink {
    fn log_intent_status_change(
        &self,
        _plan_id: &str,
        _intent_id: &IntentId,
        _old_status: &str,
        _new_status: &str,
        _reason: &str,
        _triggering_action_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
}

/// CausalChain-backed implementation that appends IntentStatusChanged actions
/// into the ledger. This is the production sink.
#[derive(Debug, Clone)]
pub struct CausalChainIntentEventSink {
    chain: Arc<Mutex<CausalChain>>,
}

impl CausalChainIntentEventSink {
    pub fn new(chain: Arc<Mutex<CausalChain>>) -> Self {
        Self { chain }
    }
}

impl IntentEventSink for CausalChainIntentEventSink {
    fn log_intent_status_change(
        &self,
        plan_id: &str,
        intent_id: &IntentId,
        old_status: &str,
        new_status: &str,
        reason: &str,
        triggering_action_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let mut guard = self
            .chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain for intent status audit".to_string()))?;

        // CausalChain.log_intent_status_change takes PlanId-ish string and other params
        guard
            .log_intent_status_change(
                &plan_id.to_string(),
                intent_id,
                old_status,
                new_status,
                reason,
                triggering_action_id,
            )
            .map(|_| ())
    }
}

/// Trait for components that want to observe Causal Chain append events.
/// Used by adapters (for example, a Working Memory ingestion sink) to
/// react to actions appended to the ledger.
pub trait CausalChainEventSink: Debug + Send + Sync {
    /// Called synchronously after an action is appended to the causal chain.
    /// Implementations should remain lightweight and non-blocking.
    fn on_action_appended(&self, action: &Action);
}

