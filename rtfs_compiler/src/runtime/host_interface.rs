//! Defines the `HostInterface` trait for decoupling the runtime from the host environment.

use crate::runtime::error::RuntimeResult;
use crate::runtime::values::Value;

/// `HostInterface` is the contract between an RTFS execution engine (like the `Evaluator`)
/// and the host environment (like the CCOS Arbiter).
///
/// This trait allows the runtime to request host-level services, such as executing a
/// capability, without being coupled to the specific implementation of the host.
pub trait HostInterface: std::fmt::Debug {
    /// Sets the context for the subsequent execution run.
    ///
    /// This must be called by the host (e.g., a Plan Executor) before evaluating
    /// the RTFS program to ensure that all generated actions have the correct
    /// provenance (plan ID, intent IDs).
    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>);

    /// Clears the execution context after a run is complete.
    fn clear_execution_context(&self);

    /// Requests that the host execute a capability with the given name and arguments.
    ///
    /// The implementation of this method is responsible for all CCOS-specific logic,
    /// including security checks, marketplace lookups, and causal chain recording.
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value>;
}
