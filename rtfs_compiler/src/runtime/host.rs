//! `RuntimeHost` - The bridge between the RTFS runtime and the CCOS host environment.

use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::host_interface::HostInterface;
use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;
use crate::ccos::causal_chain::CausalChain;
use crate::ccos::types::{Action, ExecutionResult};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Holds the contextual information for a single execution run,
/// derived from a CCOS `Plan` object.
#[derive(Clone, Debug)]
struct ExecutionContext {
    plan_id: String,
    intent_ids: Vec<String>,
}

/// `RuntimeHost` provides the execution context for CCOS capabilities.
///
/// This struct encapsulates all the logic for interacting with the CCOS host environment,
/// including the capability marketplace, causal chain, and security context. It is designed
/// to be shared between different RTFS runtimes (e.g., the AST evaluator and the IR runtime).
pub struct RuntimeHost {
    pub capability_marketplace: Arc<CapabilityMarketplace>,
    pub causal_chain: Rc<RefCell<CausalChain>>,
    pub security_context: RuntimeContext,
    execution_context: RefCell<Option<ExecutionContext>>,
}

impl RuntimeHost {
    /// Creates a new `RuntimeHost`.
    pub fn new(
        capability_marketplace: Arc<CapabilityMarketplace>,
        causal_chain: Rc<RefCell<CausalChain>>,
        security_context: RuntimeContext,
    ) -> Self {
        Self {
            capability_marketplace,
            causal_chain,
            security_context,
            execution_context: RefCell::new(None),
        }
    }

    /// Public wrapper to set the execution context before running RTFS code.
    pub fn prepare_execution(&self, plan_id: String, intent_ids: Vec<String>) {
        self.set_execution_context(plan_id, intent_ids);
    }

    /// Public wrapper to clear the execution context after running RTFS code.
    pub fn cleanup_execution(&self) {
        self.clear_execution_context();
    }
}

impl std::fmt::Debug for RuntimeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHost")
            .field("security_context", &self.security_context)
            .field("execution_context", &self.execution_context)
            .finish_non_exhaustive()
    }
}

impl HostInterface for RuntimeHost {
    /// Sets the context for the subsequent execution run.
    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>) {
        *self.execution_context.borrow_mut() = Some(ExecutionContext {
            plan_id,
            intent_ids,
        });
    }

    /// Clears the execution context after a run is complete.
    fn clear_execution_context(&self) {
        *self.execution_context.borrow_mut() = None;
    }

    /// Executes a CCOS capability.
    ///
    /// This is the central point for all capability calls from within the RTFS runtime.
    /// It performs security checks, invokes the capability through the marketplace,
    /// and records the action and its result in the causal chain using the
    /// context provided by `set_execution_context`.
    ///
    /// # Panics
    /// Panics if `set_execution_context` was not called before this method.
    fn execute_capability(
        &self,
        capability_name: &str,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        // 1. Check if the capability is allowed in the current security context.
        if !self.security_context.is_capability_allowed(capability_name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: capability_name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }

        // 2. Prepare arguments for the capability marketplace.
        let capability_args = Value::List(args.to_vec());

        // 3. Create an Action for causal chain tracking using the execution context.
        let context = self.execution_context.borrow();
        let (plan_id, intent_id) = match &*context {
            Some(ctx) => (
                ctx.plan_id.clone(),
                // For simplicity, we'll use the first intent ID. A more complex
                // system might need to track which specific intent an action serves.
                ctx.intent_ids.get(0).cloned().unwrap_or_default(),
            ),
            None => {
                // This is a programming error on the host's part. The host must
                // set the context before executing any RTFS code that might call capabilities.
                panic!("FATAL: `execute_capability` called without a valid execution context. The host must call `set_execution_context` before the run.");
            }
        };

        let mut action = Action::new_capability(
            plan_id,
            intent_id,
            capability_name.to_string(),
            args.to_vec(),
        );
        action.capability_id = Some(capability_name.to_string());

        // 4. Execute the capability via the marketplace.
        // We need to handle the async execution in a blocking way for the sync runtime.
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create async runtime: {}", e)))?;

        let result = rt.block_on(async {
            self.capability_marketplace
                .execute_capability(capability_name, &capability_args)
                .await
        });

        // 5. Record the result in the causal chain.
        let execution_result = ExecutionResult {
            success: result.is_ok(),
            value: result.as_ref().unwrap_or(&Value::Nil).clone(),
            metadata: HashMap::new(),
        };

        if let Err(e) = self.causal_chain.borrow_mut().record_result(action, execution_result) {
            // Log the error but don't fail the entire capability execution.
            // In a real-world scenario, this might go to a more robust logging system.
            eprintln!("Warning: Failed to record action result in causal chain: {:?}", e);
        }

        // 6. Return the final result of the capability execution.
        result
    }
}
