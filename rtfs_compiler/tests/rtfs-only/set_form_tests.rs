use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::ast::{Expression, Literal, Symbol};
use rtfs_compiler::runtime::error::RuntimeError;
use std::sync::Arc;
use std::collections::HashMap;

#[test]
fn test_set_with_symbol() {
    // Build required runtime collaborators using available constructors
    let module_registry = Arc::new(rtfs_compiler::runtime::module_runtime::ModuleRegistry::new());
    // Simple DummyHost defined below implements HostInterface for tests
    let host: Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> = Arc::new(DummyHost::new());
    let ev = Evaluator::new_with_defaults(module_registry, host);

    let mut env = Environment::new();

    // (set! x 42)
    let sym = Expression::Symbol(Symbol("x".to_string()));
    let lit = Expression::Literal(Literal::Integer(42));
    let expr = Expression::List(vec![Expression::Symbol(Symbol("set!".to_string())), sym.clone(), lit.clone()]);

    let res = ev.eval_expr(&expr, &mut env);
    assert!(res.is_ok());
    // After set!, x should be defined in env and equal to 42
    let val = env.lookup(&Symbol("x".to_string())).expect("x not found");
    assert_eq!(format!("{}", val), "42");
}

#[test]
fn test_set_with_keyword() {
    let module_registry = Arc::new(rtfs_compiler::runtime::module_runtime::ModuleRegistry::new());
    let host: Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> = Arc::new(DummyHost::new());
    let ev = Evaluator::new_with_defaults(module_registry, host);

    let mut env = Environment::new();

    // (set! :k "v") -> should coerce :k -> symbol k and store
    let kw = Expression::Literal(Literal::Keyword(rtfs_compiler::ast::Keyword("k".to_string())));
    let lit = Expression::Literal(Literal::String("v".to_string()));
    let expr = Expression::List(vec![Expression::Symbol(Symbol("set!".to_string())), kw.clone(), lit.clone()]);

    let res = ev.eval_expr(&expr, &mut env);
    assert!(res.is_ok());
    // After set!, symbol "k" should be present
    let val = env.lookup(&Symbol("k".to_string())).expect("k not found");
    assert_eq!(format!("{}", val), "\"v\"");
}

#[test]
fn test_set_invalid_first_arg() {
    let module_registry = Arc::new(rtfs_compiler::runtime::module_runtime::ModuleRegistry::new());
    let host: Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> = Arc::new(DummyHost::new());
    let ev = Evaluator::new_with_defaults(module_registry, host);

    let mut env = Environment::new();

    // (set! 123 1) -> first arg numeric literal should error
    let num = Expression::Literal(Literal::Integer(123));
    let lit = Expression::Literal(Literal::Integer(1));
    let expr = Expression::List(vec![Expression::Symbol(Symbol("set!".to_string())), num, lit]);

    let res = ev.eval_expr(&expr, &mut env);
    assert!(res.is_err());
    match res.unwrap_err() {
        RuntimeError::TypeError { operation, .. } => assert_eq!(operation, "set!".to_string()),
        other => panic!("unexpected error: {:?}", other),
    }
}

// Minimal DummyHost implementation for tests
#[derive(Debug)]
struct DummyHost;

impl DummyHost {
    fn new() -> Self { DummyHost }
}

impl rtfs_compiler::runtime::host_interface::HostInterface for DummyHost {
    fn execute_capability(&self, _name: &str, _args: &[rtfs_compiler::runtime::values::Value]) -> rtfs_compiler::runtime::error::RuntimeResult<rtfs_compiler::runtime::values::Value> {
        // Return a nil value for capability calls in tests
        Ok(rtfs_compiler::runtime::values::Value::Nil)
    }

    fn notify_step_started(&self, _step_name: &str) -> rtfs_compiler::runtime::error::RuntimeResult<String> {
        Ok("test-step".to_string())
    }

    fn notify_step_completed(&self, _step_action_id: &str, _result: &rtfs_compiler::ccos::types::ExecutionResult) -> rtfs_compiler::runtime::error::RuntimeResult<()> {
        Ok(())
    }

    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> rtfs_compiler::runtime::error::RuntimeResult<()> {
        Ok(())
    }

    fn set_execution_context(&self, _plan_id: String, _intent_ids: Vec<String>, _parent_action_id: String) {
        // no-op
    }

    fn clear_execution_context(&self) {
        // no-op
    }

    fn set_step_exposure_override(&self, _expose: bool, _context_keys: Option<Vec<String>>) {
        // no-op
    }

    fn clear_step_exposure_override(&self) {
        // no-op
    }

    fn get_context_value(&self, _key: &str) -> Option<rtfs_compiler::runtime::values::Value> {
        None
    }

    // Use default prepare/cleanup implementations
}
