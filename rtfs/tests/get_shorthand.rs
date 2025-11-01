use rtfs::ast::{Keyword, MapKey};
use rtfs::parser;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::values::Value;
use std::sync::Arc;

#[derive(Debug)]
struct StubHost;

impl HostInterface for StubHost {
    fn notify_step_started(
        &self,
        _name: &str,
    ) -> Result<String, rtfs::runtime::error::RuntimeError> {
        Ok("step-1".to_string())
    }
    fn notify_step_completed(
        &self,
        _id: &str,
        _res: &rtfs::runtime::stubs::ExecutionResultStruct,
    ) -> Result<(), rtfs::runtime::error::RuntimeError> {
        Ok(())
    }
    fn notify_step_failed(
        &self,
        _id: &str,
        _err: &str,
    ) -> Result<(), rtfs::runtime::error::RuntimeError> {
        Ok(())
    }
    fn get_context_value(&self, _k: &str) -> Option<rtfs::runtime::values::Value> {
        None
    }
    fn execute_capability(
        &self,
        cap: &str,
        args: &[rtfs::runtime::values::Value],
    ) -> Result<rtfs::runtime::values::Value, rtfs::runtime::error::RuntimeError>
    {
        match cap {
            "ccos.state.kv.put" => Ok(rtfs::runtime::values::Value::Nil), // Put operations return nil
            "ccos.state.kv.get" => {
                // For get operations, check if we're looking for key "k"
                if args.len() >= 1 {
                    if let rtfs::runtime::values::Value::Map(map) = &args[0] {
                        if let Some(rtfs::runtime::values::Value::String(key)) =
                            map.get(&MapKey::Keyword(Keyword("key".to_string())))
                        {
                            if key == "k" {
                                return Ok(rtfs::runtime::values::Value::String(
                                    "v".to_string(),
                                ));
                            }
                        }
                    }
                }
                Ok(rtfs::runtime::values::Value::Nil)
            }
            _ => Ok(rtfs::runtime::values::Value::Nil),
        }
    }
    fn set_execution_context(
        &self,
        _plan_id: String,
        _intent_ids: Vec<String>,
        _parent_action_id: String,
    ) {
    }
    fn clear_execution_context(&self) {}
    fn set_step_exposure_override(&self, _expose: bool, _keys: Option<Vec<String>>) {}
    fn clear_step_exposure_override(&self) {}
}

#[test]
fn test_get_shorthand_and_builtin_get() -> Result<(), rtfs::runtime::error::RuntimeError> {
    // prepare evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    let host = Arc::new(StubHost);
    let mut ev = Evaluator::new_with_defaults(module_registry, host);

    // Test shorthand get with host capabilities (set! removed in migration)
    let set_prog = "(do (step \"set\" (call :ccos.state.kv.put {:key \"k\" :value \"v\"})))";
    let set_items = parser::parse(set_prog).map_err(|e| {
        rtfs::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e))
    })?;
    let _ = match ev.eval_toplevel(&set_items)? {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(_) => {}
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Host call required in pure test".to_string(),
            ));
        }
    };

    let get_prog = "(do (step \"get\" (call :ccos.state.kv.get {:key \"k\"})))";
    let get_items = parser::parse(get_prog).map_err(|e| {
        rtfs::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e))
    })?;
    let result = ev.eval_toplevel(&get_items)?;
    let val = match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Host call required in pure test".to_string(),
            ));
        }
    };
    println!("DEBUG val = {:?}", val);
    // last evaluation should be the value of (call :ccos.state.kv.get {:key "k"})
    assert!(matches!(val, Value::String(ref s) if s == "v"));

    // Test builtin get on a map (set! removed in migration)
    let program2 =
        "(do (step \"mkt\" (let [m (hash-map :a 1)] m)) (step \"read\" (get (hash-map :a 1) :a)))";
    let items2 = parser::parse(program2).map_err(|e| {
        rtfs::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e))
    })?;
    let result2 = ev.eval_toplevel(&items2)?;
    let val2 = match result2 {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Host call required in pure test".to_string(),
            ));
        }
    };
    println!("DEBUG val2 = {:?}", val2);
    assert!(matches!(val2, Value::Integer(1)));

    Ok(())
}
