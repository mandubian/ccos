use rtfs_compiler::parser;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
struct StubHost;

impl HostInterface for StubHost {
    fn notify_step_started(&self, _name: &str) -> Result<String, rtfs_compiler::runtime::error::RuntimeError> { Ok("step-1".to_string()) }
    fn notify_step_completed(&self, _id: &str, _res: &rtfs_compiler::ccos::types::ExecutionResult) -> Result<(), rtfs_compiler::runtime::error::RuntimeError> { Ok(()) }
    fn notify_step_failed(&self, _id: &str, _err: &str) -> Result<(), rtfs_compiler::runtime::error::RuntimeError> { Ok(()) }
    fn get_context_value(&self, _k: &str) -> Option<rtfs_compiler::runtime::values::Value> { None }
    fn execute_capability(&self, _cap: &str, _args: &[rtfs_compiler::runtime::values::Value]) -> Result<rtfs_compiler::runtime::values::Value, rtfs_compiler::runtime::error::RuntimeError> { Ok(rtfs_compiler::runtime::values::Value::Nil) }
    fn set_execution_context(&self, _plan_id: String, _intent_ids: Vec<String>, _parent_action_id: String) { }
    fn clear_execution_context(&self) { }
    fn set_step_exposure_override(&self, _expose: bool, _keys: Option<Vec<String>>) { }
    fn clear_step_exposure_override(&self) { }
}

#[test]
fn test_get_shorthand_and_builtin_get() -> Result<(), rtfs_compiler::runtime::error::RuntimeError> {
    // prepare evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    let host = Arc::new(StubHost);
    let mut ev = Evaluator::new_with_defaults(module_registry, host);

    // Test shorthand get after set!
    let set_prog = "(do (step \"set\" (set! :k \"v\")))";
    let set_items = parser::parse(set_prog).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let _ = ev.eval_toplevel(&set_items)?;

    let get_prog = "(do (step \"get\" (get :k)))";
    let get_items = parser::parse(get_prog).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let val = ev.eval_toplevel(&get_items)?;
    println!("DEBUG val = {:?}", val);
    // last evaluation should be the value of (get :k)
    assert!(matches!(val, Value::String(ref s) if s == "v"));

    // Test builtin get on a map
    let program2 = "(do (step \"mkt\" (set! :m (hash-map :a 1))) (step \"read\" (get (get :m) :a)))";
    let items2 = parser::parse(program2).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let val2 = ev.eval_toplevel(&items2)?;
    println!("DEBUG val2 = {:?}", val2);
    assert!(matches!(val2, Value::Integer(1)));

    Ok(())
}
