use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::Symbol;
use std::sync::Arc;

// Minimal HostInterface stub
struct StubHost;
impl std::fmt::Debug for StubHost { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "StubHost") } }

impl HostInterface for StubHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // Mock the state capabilities to return expected values
        match name {
            "ccos.state.kv.put" => {
                // For kv.put, return the value that was stored
                if let Some(Value::Map(map)) = args.first() {
                    if let Some(Value::String(value)) = map.get(&rtfs_compiler::ast::MapKey::Keyword(rtfs_compiler::ast::Keyword("value".to_string()))) {
                        return Ok(Value::String(value.clone()));
                    }
                }
                Ok(Value::Nil)
            }
            "ccos.state.kv.get" => {
                // For kv.get, return the stored value
                if let Some(Value::Map(map)) = args.first() {
                    if let Some(Value::String(key)) = map.get(&rtfs_compiler::ast::MapKey::Keyword(rtfs_compiler::ast::Keyword("key".to_string()))) {
                        if key == "k" {
                            return Ok(Value::String("v".to_string()));
                        }
                    }
                }
                Ok(Value::Nil)
            }
            _ => Ok(Value::Nil)
        }
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
    let host = Arc::new(StubHost);

    let mut ev = Evaluator::new_with_defaults(module_registry, host);

    // Test host capability flow: store then retrieve
    let store_program = r#"(do (step "store" (call :ccos.state.kv.put {:key "k" :value "v"})))"#;
    let retrieve_program = r#"(do (step "retrieve" (call :ccos.state.kv.get {:key "k"})))"#;

    // Parse and evaluate store
    let store_items = rtfs_compiler::parser::parse(store_program).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let store_result = ev.eval_toplevel(&store_items)?;
    
    // Parse and evaluate retrieve
    let retrieve_items = rtfs_compiler::parser::parse(retrieve_program).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    let retrieve_result = ev.eval_toplevel(&retrieve_items)?;

    // Check that retrieve returns the stored value
    let got = match retrieve_result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Expected complete result, got RequiresHost");
        }
    };
    
    if let Value::String(s) = got {
        assert_eq!(s, "v");
    } else {
        panic!("expected string value from get :k, got: {:?}", got);
    }

    Ok(())
}
