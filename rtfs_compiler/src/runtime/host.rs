//! Runtime Host
//!
//! The concrete implementation of the `HostInterface` that connects the RTFS runtime
//! to the CCOS stateful components like the Causal Chain and Capability Marketplace.

use std::sync::{Arc, Mutex, MutexGuard};
use std::cell::RefCell;

use crate::runtime::host_interface::HostInterface;
use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use crate::ccos::causal_chain::CausalChain;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::security::RuntimeContext;
use crate::ccos::types::{Action, ActionType, ExecutionResult};

#[derive(Debug, Clone)]
struct ExecutionContext {
    plan_id: String,
    intent_ids: Vec<String>,
    parent_action_id: String,
}

/// The RuntimeHost is the bridge between the pure RTFS runtime and the stateful CCOS world.
pub struct RuntimeHost {
    causal_chain: Arc<Mutex<CausalChain>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
    // The execution context is stored in a RefCell to allow for interior mutability
    // during the evaluation of a plan.
    execution_context: RefCell<Option<ExecutionContext>>,
}

impl RuntimeHost {
    pub fn new(
        causal_chain: Arc<Mutex<CausalChain>>,
        capability_marketplace: Arc<CapabilityMarketplace>,
        security_context: RuntimeContext,
    ) -> Self {
        Self {
            causal_chain,
            capability_marketplace,
            security_context,
            execution_context: RefCell::new(None),
        }
    }

    /// Sets the context for a new plan execution.
    pub fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>, parent_action_id: String) {
        *self.execution_context.borrow_mut() = Some(ExecutionContext {
            plan_id,
            intent_ids,
            parent_action_id,
        });
    }

    /// Clears the execution context after a plan has finished.
    pub fn clear_execution_context(&self) {
        *self.execution_context.borrow_mut() = None;
    }

    fn get_causal_chain(&self) -> RuntimeResult<MutexGuard<CausalChain>> {
        self.causal_chain.lock().map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))
    }

    fn get_context(&self) -> RuntimeResult<std::cell::Ref<ExecutionContext>> {
        let context_ref = self.execution_context.borrow();
        if context_ref.is_none() {
            return Err(RuntimeError::Generic("FATAL: Host method called without a valid execution context".to_string()));
        }
        Ok(std::cell::Ref::map(context_ref, |c| c.as_ref().unwrap()))
    }
}

impl HostInterface for RuntimeHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // 1. Security Validation
        if !self.security_context.is_capability_allowed(name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }

        let context = self.get_context()?;
        let capability_args = Value::List(args.to_vec());

        // 2. Create and log the CapabilityCall action
        let action = Action::new(
            ActionType::CapabilityCall,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(context.parent_action_id.clone()))
        .with_name(name)
        .with_arguments(&args);

        let action_id = self.get_causal_chain()?.append(&action)?;

        // 3. Execute the capability via the marketplace
        // Note: This is a blocking call to bridge the async marketplace with the sync evaluator.
        // A production system might use a more sophisticated async bridge.
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create Tokio runtime: {}", e)))?;

        let result = rt.block_on(async {
            self.capability_marketplace.execute_capability(name, &capability_args).await
        });

        // 4. Log the result to the Causal Chain
        let execution_result = match &result {
            Ok(value) => ExecutionResult { success: true, value: value.clone(), metadata: Default::default() },
            Err(e) => ExecutionResult { success: false, value: Value::Nil, metadata: Default::default() }.with_error(&e.to_string()),
        };

        self.get_causal_chain()?.record_result(action, execution_result)?;

        result
    }

    fn notify_step_started(&self, step_name: &str) -> RuntimeResult<String> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepStarted,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(context.parent_action_id.clone()))
        .with_name(step_name);

        self.get_causal_chain()?.append(&action)
    }

    fn notify_step_completed(&self, step_action_id: &str, result: &ExecutionResult) -> RuntimeResult<()> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepCompleted,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(step_action_id.to_string()))
        .with_result(result.clone());

        self.get_causal_chain()?.append(&action)?;
        Ok(())
    }

    fn notify_step_failed(&self, step_action_id: &str, error: &str) -> RuntimeResult<()> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepFailed,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(step_action_id.to_string()))
        .with_error(error);

        self.get_causal_chain()?.append(&action)?;
        Ok(())
    }

    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>, parent_action_id: String) {
        self.set_execution_context(plan_id, intent_ids, parent_action_id);
    }

    fn clear_execution_context(&self) {
        self.clear_execution_context();
    }
}

impl std::fmt::Debug for RuntimeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHost")
         .field("security_context", &self.security_context)
         .field("execution_context", &self.execution_context.borrow())
         .finish()
    }
}