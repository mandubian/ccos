//! Event sink trait for Causal Chain observers
//!
//! Provides a lightweight notification mechanism for Causal Chain append events.
//! This allows Working Memory and other components to stay synchronized with
//! the chain without modifying core ledger logic.

use crate::ccos::types::Action;
use std::fmt::Debug;

/// Trait for components that want to observe Causal Chain events
pub trait CausalChainEventSink: Send + Sync + Debug {
    /// Called when a new action is appended to the causal chain
    fn on_action_appended(&self, action: &Action);
}
