//! Host Interface
//!
//! Defines the trait that bridges the RTFS runtime (which is pure) and the
//! CCOS host environment (which manages state, security, and capabilities).

use crate::runtime::error::RuntimeResult;
use crate::runtime::stubs::ExecutionResultStruct;
use crate::runtime::values::Value;

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
    fn notify_step_completed(
        &self,
        step_action_id: &str,
        result: &ExecutionResultStruct,
    ) -> RuntimeResult<()>;

    /// Notifies the host that a plan step has failed.
    fn notify_step_failed(&self, step_action_id: &str, error: &str) -> RuntimeResult<()>;

    /// Sets the execution context for subsequent capability calls.
    /// This is required before calling execute_capability.
    fn set_execution_context(
        &self,
        plan_id: String,
        intent_ids: Vec<String>,
        parent_action_id: String,
    );

    /// Clears the execution context after a plan has finished.
    fn clear_execution_context(&self);

    /// Step-scoped override: request read-only context exposure for capability calls within the step
    /// If `expose` is false, suppress exposure even if policy allows it. If `context_keys` is provided,
    /// only those keys will be included in the snapshot.
    fn set_step_exposure_override(&self, expose: bool, context_keys: Option<Vec<String>>);

    /// Clear the most recent step exposure override (called on step exit)
    fn clear_step_exposure_override(&self);
    /// Returns a value from the current execution context, if available.
    /// Common keys include:
    /// - "plan-id"
    /// - "intent-id"
    /// - "intent-ids"
    /// - "parent-action-id"
    /// Returns None when the value is not available.
    /// 
    /// This method should check both step-scoped context (from current step) and
    /// plan-scoped context, in that order.
    fn get_context_value(&self, key: &str) -> Option<Value>;
    
    /// Sets a step-scoped context value (optional - for hosts that support step-local storage).
    /// If the host doesn't support step-scoped context, this is a no-op.
    /// Step-scoped values are automatically cleared when the step exits.
    fn set_step_context_value(&self, _key: String, _value: Value) -> RuntimeResult<()> {
        // Default implementation: no-op (step context not required for basic hosts)
        Ok(())
    }

    /// Optional lifecycle helper: prepare host for a new execution (called before plan start).
    /// Many callers in the codebase call `prepare_execution(plan_id, intent_ids)` as a convenience
    /// wrapper around `set_execution_context` — provide a default implementation that sets a
    /// sensible parent_action_id and returns Ok(()) so implementations may override if needed.
    fn prepare_execution(&self, plan_id: String, intent_ids: Vec<String>) -> RuntimeResult<()> {
        // Default parent action id when not provided by the caller
        self.set_execution_context(plan_id, intent_ids, "0".to_string());
        Ok(())
    }

    /// Optional lifecycle helper: cleanup host after execution (called after plan end).
    /// Default implementation clears the execution context and returns Ok(()).
    fn cleanup_execution(&self) -> RuntimeResult<()> {
        self.clear_execution_context();
        Ok(())
    }
}
