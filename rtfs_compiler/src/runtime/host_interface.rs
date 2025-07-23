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
}