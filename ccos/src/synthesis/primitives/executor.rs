use std::sync::Arc;

use rtfs::compiler::expander::MacroExpander;
use rtfs::parser;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::execution_outcome::ExecutionOutcome;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::secure_stdlib::SecureStandardLibrary;
use rtfs::runtime::stubs::ExecutionResultStruct;
use rtfs::runtime::values::Value;

#[derive(Debug)]
struct RestrictedHost;

impl HostInterface for RestrictedHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        if name == "ccos.io.println" || name == "io.println" {
            let message = args
                .iter()
                .map(|v| format!("{}", v))
                .collect::<Vec<_>>()
                .join(" ");
            println!("{}", message);
            return Ok(Value::Nil);
        }
        Err(RuntimeError::SecurityViolation {
            operation: "execute_capability".to_string(),
            capability: name.to_string(),
            context: "Restricted primitive execution forbids host calls".to_string(),
        })
    }

    fn notify_step_started(&self, _step_name: &str) -> RuntimeResult<String> {
        Ok("restricted-step".to_string())
    }

    fn notify_step_completed(
        &self,
        _step_action_id: &str,
        _result: &ExecutionResultStruct,
    ) -> RuntimeResult<()> {
        Ok(())
    }

    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> RuntimeResult<()> {
        Ok(())
    }

    fn set_execution_context(
        &self,
        _plan_id: String,
        _intent_ids: Vec<String>,
        _parent_action_id: String,
    ) {
    }

    fn clear_execution_context(&self) {}

    fn set_step_exposure_override(&self, _expose: bool, _context_keys: Option<Vec<String>>) {}

    fn clear_step_exposure_override(&self) {}

    fn get_context_value(&self, _key: &str) -> Option<Value> {
        None
    }
}

/// Executes primitive RTFS functions inside a SecureStandardLibrary-only environment.
pub struct RestrictedRtfsExecutor {
    evaluator: Evaluator,
}

impl RestrictedRtfsExecutor {
    pub fn new() -> Self {
        let module_registry = Arc::new(ModuleRegistry::new());
        let host = Arc::new(RestrictedHost);
        let macro_expander = MacroExpander::default();
        let evaluator =
            Evaluator::new_with_defaults(module_registry.clone(), host.clone(), macro_expander);

        Self { evaluator }
    }

    /// Evaluate an RTFS `(fn [input] ...)` against a provided argument in the restricted runtime.
    pub fn evaluate(&self, rtfs_function: &str, input: Value) -> RuntimeResult<Value> {
        let expr = parser::parse_expression(rtfs_function).map_err(|err| {
            RuntimeError::Generic(format!("Failed to parse synthesized primitive: {:?}", err))
        })?;

        let mut env = rtfs::runtime::stdlib::StandardLibrary::create_global_environment();

        let func_value = match self.evaluator.evaluate_with_env(&expr, &mut env)? {
            ExecutionOutcome::Complete(value) => value,
            ExecutionOutcome::RequiresHost(call) => {
                return Err(RuntimeError::SecurityViolation {
                    operation: "evaluate_primitive".to_string(),
                    capability: call.capability_id,
                    context: "Restricted primitive execution forbids host calls".to_string(),
                })
            }
        };

        let call_outcome = self
            .evaluator
            .call_function(func_value, &[input], &mut env)?;

        match call_outcome {
            ExecutionOutcome::Complete(value) => Ok(value),
            ExecutionOutcome::RequiresHost(call) => Err(RuntimeError::SecurityViolation {
                operation: "call_primitive".to_string(),
                capability: call.capability_id,
                context: "Restricted primitive execution forbids host calls".to_string(),
            }),
        }
    }
}

impl Default for RestrictedRtfsExecutor {
    fn default() -> Self {
        Self::new()
    }
}
