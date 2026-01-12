use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::ir_runtime::IrRuntime;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::*;
use std::sync::Arc;

#[derive(Debug)]
struct StubHost;

impl HostInterface for StubHost {
    fn execute_capability(
        &self,
        name: &str,
        _args: &[Value],
    ) -> rtfs::runtime::error::RuntimeResult<Value> {
        match name {
            "cap.good-int" => Ok(Value::Integer(42)),
            "cap.bad-int" => Ok(Value::String("not-an-int".to_string())),
            "cap.good-map" => {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword::new("a")),
                    Value::Integer(1),
                );
                Ok(Value::Map(m))
            }
            _ => Ok(Value::Nil),
        }
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
    fn get_context_value(&self, _key: &str) -> Option<Value> {
        None
    }
}

fn run_ir_expr(
    code: &str,
    host: Arc<dyn HostInterface>,
) -> Result<Value, rtfs::runtime::error::RuntimeError> {
    let parsed = parser::parse(code).expect("Should parse successfully");
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr.clone(),
        other => panic!("Expected expression, got: {:?}", other),
    };

    let module_registry = Arc::new(ModuleRegistry::new());
    rtfs::runtime::stdlib::load_stdlib(&module_registry).expect("Should load stdlib");

    let mut converter = rtfs::ir::converter::IrConverter::new();
    let ir_node = converter
        .convert_expression(expr)
        .expect("IR conversion should succeed");

    let security_context = RuntimeContext::pure();
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
fn ir_runtime_let_annotation_checked_cast_success() {
    let host: Arc<dyn HostInterface> = Arc::new(StubHost);
    let v = run_ir_expr(r#"(let [x :Int (call :cap.good-int)] x)"#, host).unwrap();
    assert_eq!(v, Value::Integer(42));
}

#[test]
fn ir_runtime_let_annotation_checked_cast_failure() {
    let host: Arc<dyn HostInterface> = Arc::new(StubHost);
    let err = run_ir_expr(r#"(let [x :Int (call :cap.bad-int)] x)"#, host).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("Type mismatch")
            || msg.contains("TypeValidationError")
            || msg.contains("let x"),
        "unexpected error message: {}",
        msg
    );
}

#[test]
fn ir_runtime_let_annotation_structural_map_checked() {
    let host: Arc<dyn HostInterface> = Arc::new(StubHost);
    // Expect a record-like map containing :a Int
    let v = run_ir_expr(r#"(let [m :[:map [:a Int]] (call :cap.good-map)] m)"#, host).unwrap();
    match v {
        Value::Map(map) => {
            assert!(map.contains_key(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword::new("a"))));
        }
        other => panic!("Expected map, got {:?}", other),
    }
}
