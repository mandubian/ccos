use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::delegation::StaticDelegationEngine;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::Symbol;
use std::sync::Arc;
use std::collections::HashMap;

// Minimal HostInterface stub
struct StubHost;
impl std::fmt::Debug for StubHost { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "StubHost") } }

impl HostInterface for StubHost {
    fn execute_capability(&self, _name: &str, _args: &[Value]) -> RuntimeResult<Value> {
        Ok(Value::Nil)
    }

    fn notify_step_started(&self, _step_name: &str) -> RuntimeResult<String> {
        Ok("step-0".to_string())
    }

    fn notify_step_completed(&self, _step_action_id: &str, _result: &rtfs_compiler::ccos::types::ExecutionResult) -> RuntimeResult<()> {
        Ok(())
    }

    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> RuntimeResult<()> {
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

    fn get_context_value(&self, _key: &str) -> Option<Value> {
        None
    }
}

#[tokio::test]
async fn test_set_keyword_coercion() -> RuntimeResult<()> {
    // Create minimal dependencies for evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    // Use the built-in static delegation engine with empty policy
    let delegation_engine = Arc::new(StaticDelegationEngine::new_empty());
    let host = Arc::new(StubHost);

    let mut ev = Evaluator::new_with_defaults(module_registry, delegation_engine, host);

    // Program: (do (set! :k "v"))
    let program = r#"(do (set! :k "v"))"#;

    // Parse and evaluate via public parser + evaluator
    let items = rtfs_compiler::parser::parse(program).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let _ = ev.eval_toplevel(&items)?;

    // The last evaluation should leave the evaluator environment with k -> "v"
    let got = ev.env.lookup(&Symbol("k".to_string())).unwrap_or(Value::Nil);
    if let Value::String(s) = got {
        assert_eq!(s, "v");
    } else {
        panic!("expected string value from get :k, got: {:?}", got);
    }

    Ok(())
}
