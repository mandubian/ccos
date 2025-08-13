//! Host Interface
//!
//! Defines the trait that bridges the RTFS runtime (which is pure) and the
//! CCOS host environment (which manages state, security, and capabilities).

use crate::runtime::values::Value;
use crate::runtime::error::RuntimeResult;
use crate::ccos::types::ExecutionResult;

/// The HostInterface provides the contract between the RTFS runtime and the CCOS host.
pub trait HostInterface: std::fmt::Debug {
    /// Executes a capability through the CCOS infrastructure.
    /// This is the primary entry point for the runtime to perform external actions.
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value>;

    /// Notifies the host that a plan step has started.
    /// The host is responsible for logging this to the Causal Chain.
    /// Returns the action_id of the created "PlanStepStarted" action.
    fn notify_step_started(&self, step_name: &str) -> RuntimeResult<String>;

    /// Notifies the host that a plan step has completed successfully.
    fn notify_step_completed(&self, step_action_id: &str, result: &ExecutionResult) -> RuntimeResult<()>;

    /// Notifies the host that a plan step has failed.
    fn notify_step_failed(&self, step_action_id: &str, error: &str) -> RuntimeResult<()>;

    /// Sets the execution context for subsequent capability calls.
    /// This is required before calling execute_capability.
    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>, parent_action_id: String);

    /// Clears the execution context after a plan has finished.
    fn clear_execution_context(&self);

    /// Step-scoped override: request read-only context exposure for capability calls within the step
    /// If `expose` is false, suppress exposure even if policy allows it. If `context_keys` is provided,
    /// only those keys will be included in the snapshot.
    fn set_step_exposure_override(&self, expose: bool, context_keys: Option<Vec<String>>);

    /// Clear the most recent step exposure override (called on step exit)
    fn clear_step_exposure_override(&self);
}