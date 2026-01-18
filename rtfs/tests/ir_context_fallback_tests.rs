use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::ir_runtime::IrRuntime;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::{parser, TopLevel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct ContextHost {
    values: Mutex<HashMap<String, Value>>,
}

impl ContextHost {
    fn new(values: HashMap<String, Value>) -> Self {
        Self {
            values: Mutex::new(values),
        }
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.values.lock().ok()?.get(key).cloned()
    }
}

impl HostInterface for ContextHost {
    fn execute_capability(
        &self,
        _name: &str,
        _args: &[Value],
    ) -> rtfs::runtime::error::RuntimeResult<Value> {
        Ok(Value::Nil)
    }

    fn notify_step_started(&self, _step_name: &str) -> rtfs::runtime::error::RuntimeResult<String> {
        Ok("step-1".to_string())
    }

    fn notify_step_completed(
        &self,
        _step_action_id: &str,
        _result: &rtfs::runtime::stubs::ExecutionResultStruct,
    ) -> rtfs::runtime::error::RuntimeResult<()> {
        Ok(())
    }

    fn notify_step_failed(
        &self,
        _step_action_id: &str,
        _error: &str,
    ) -> rtfs::runtime::error::RuntimeResult<()> {
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

    fn get_context_value(&self, key: &str) -> Option<Value> {
        self.get(key)
    }

    fn set_step_context_value(&self, key: String, value: Value) -> rtfs::runtime::error::RuntimeResult<()> {
        if let Ok(mut guard) = self.values.lock() {
            guard.insert(key, value);
        }
        Ok(())
    }
}

fn run_ir_expr_with_context(
    code: &str,
    host: Arc<dyn HostInterface>,
    security_context: RuntimeContext,
) -> Result<Value, rtfs::runtime::error::RuntimeError> {
    let parsed = parser::parse(code).expect("Should parse successfully");
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr.clone(),
        other => panic!("Expected expression, got: {:?}", other),
    };

    let module_registry = Arc::new(ModuleRegistry::new());
    rtfs::runtime::stdlib::load_stdlib(&module_registry).expect("Should load stdlib");

    let mut converter = rtfs::ir::converter::IrConverter::with_module_registry(&module_registry);
    let ir_node = converter
        .convert_expression(expr)
        .expect("IR conversion should succeed");

    let mut runtime = IrRuntime::new(host, security_context);

    let mut env = rtfs::runtime::environment::IrEnvironment::with_stdlib(&module_registry)
        .expect("Should create environment with stdlib");

    let outcome = runtime.execute_node(&ir_node, &mut env, false, &module_registry)?;
    match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(v) => Ok(v),
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            Err(rtfs::runtime::error::RuntimeError::Generic(
                "Unexpected host yield in test".to_string(),
            ))
        }
    }
}

#[test]
fn ir_symbol_falls_back_to_cross_plan_params() {
    let host: Arc<dyn HostInterface> = Arc::new(ContextHost::default());

    let mut security_context = RuntimeContext::pure();
    security_context.add_cross_plan_param("x".to_string(), Value::Integer(7));

    let v = run_ir_expr_with_context("x", host, security_context).unwrap();
    assert_eq!(v, Value::Integer(7));
}

#[test]
fn ir_symbol_falls_back_to_host_context() {
    let mut init = HashMap::new();
    init.insert("plan-id".to_string(), Value::String("p-123".to_string()));

    let host: Arc<dyn HostInterface> = Arc::new(ContextHost::new(init));
    let security_context = RuntimeContext::pure();

    let v = run_ir_expr_with_context("plan-id", host, security_context).unwrap();
    assert_eq!(v, Value::String("p-123".to_string()));
}

#[test]
fn ir_set_context_value_delegates_to_host() {
    let host_impl = Arc::new(ContextHost::default());
    let host: Arc<dyn HostInterface> = host_impl.clone();

    let security_context = RuntimeContext::pure();
    let runtime = IrRuntime::new(host, security_context);

    runtime
        .set_context_value("k".to_string(), Value::String("v".to_string()))
        .unwrap();

    assert_eq!(host_impl.get("k"), Some(Value::String("v".to_string())));
}

#[test]
fn ir_context_set_and_get_builtins_work() {
    let host_impl = Arc::new(ContextHost::default());
    let host: Arc<dyn HostInterface> = host_impl.clone();

    let security_context = RuntimeContext::pure();
    let v = run_ir_expr_with_context(
        "(do (context/set :answer 42) (context/get :answer))",
        host,
        security_context,
    )
    .unwrap();
    assert_eq!(v, Value::Integer(42));

    assert_eq!(host_impl.get("answer"), Some(Value::Integer(42)));
}
