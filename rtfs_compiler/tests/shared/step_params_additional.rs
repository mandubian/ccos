use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::evaluator::Evaluator;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

// Focused tests for :params behavior

#[test]
fn step_params_evaluation_error_prevents_body() -> Result<(), String> {
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
    let stdlib_env = StandardLibrary::create_global_environment();
    let security_context = RuntimeContext::pure();

    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    let causal_chain = Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()));
    let host = Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, security_context.clone()));

    let mut evaluator = Evaluator::with_environment(
        module_registry,
        stdlib_env,
        security_context,
        host,
    );

    evaluator.context_manager.borrow_mut().initialize(Some("root".to_string()));
    evaluator.host.set_execution_context("plan1".to_string(), vec!["intent1".to_string()], "root".to_string());

    use rtfs_compiler::ast::{Expression, Literal, Symbol, Keyword, MapKey, DoExpr, TopLevel};
    let mut params_map: std::collections::HashMap<MapKey, Expression> = std::collections::HashMap::new();
    // undefined symbol should cause eval error
    params_map.insert(MapKey::String("a".to_string()), Expression::Symbol(Symbol("no_such_var".to_string())));

    let step_expr = Expression::List(vec![
        Expression::Symbol(Symbol("step".to_string())),
        Expression::Literal(Literal::String("s1".to_string())),
        Expression::Literal(Literal::Keyword(Keyword("params".to_string()))),
        Expression::Map(params_map),
        Expression::List(vec![Expression::Symbol(Symbol("get".to_string())), Expression::Symbol(Symbol("%params".to_string())), Expression::Literal(Literal::String("a".to_string()))]),
    ]);

    let do_expr = Expression::Do(DoExpr { expressions: vec![step_expr] });
    let program = vec![TopLevel::Expression(do_expr)];

    let res = evaluator.eval_toplevel(&program);
    assert!(res.is_err(), "expected parameter evaluation to fail");
    let err = format!("{:?}", res.err().unwrap());
    assert!(err.contains("UndefinedSymbol") || err.contains("ParamBinding") || err.contains("Generic"), "unexpected error: {}", err);
    Ok(())
}

#[test]
fn step_params_non_string_key_rejected() -> Result<(), String> {
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
    let stdlib_env = StandardLibrary::create_global_environment();
    let security_context = RuntimeContext::pure();

    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    let causal_chain = Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()));
    let host = Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, security_context.clone()));

    let mut evaluator = Evaluator::with_environment(
        module_registry,
        stdlib_env,
        security_context,
        host,
    );

    evaluator.context_manager.borrow_mut().initialize(Some("root".to_string()));
    evaluator.host.set_execution_context("plan1".to_string(), vec!["intent1".to_string()], "root".to_string());

    use rtfs_compiler::ast::{Expression, Literal, Symbol, Keyword, MapKey, DoExpr, TopLevel};
    let mut params_map: std::collections::HashMap<MapKey, Expression> = std::collections::HashMap::new();
    // Use a non-string key (keyword) which should be rejected
    params_map.insert(MapKey::Keyword(Keyword("k".to_string())), Expression::Literal(Literal::Integer(1)));

    let step_expr = Expression::List(vec![
        Expression::Symbol(Symbol("step".to_string())),
        Expression::Literal(Literal::String("s2".to_string())),
        Expression::Literal(Literal::Keyword(Keyword("params".to_string()))),
        Expression::Map(params_map),
        Expression::List(vec![Expression::Symbol(Symbol("get".to_string())), Expression::Symbol(Symbol("%params".to_string())), Expression::Literal(Literal::String("k".to_string()))]),
    ]);

    let do_expr = Expression::Do(DoExpr { expressions: vec![step_expr] });
    let program = vec![TopLevel::Expression(do_expr)];

    let res = evaluator.eval_toplevel(&program);
    assert!(res.is_err(), "expected non-string key to be rejected");
    let err = format!("{:?}", res.err().unwrap());
    assert!(err.contains("InvalidArguments") || err.contains("string keys"), "unexpected error: {}", err);
    Ok(())
}

#[test]
fn step_params_shadowing_for_nested_steps() -> Result<(), String> {
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
    let stdlib_env = StandardLibrary::create_global_environment();
    let security_context = RuntimeContext::pure();

    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    let causal_chain = Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()));
    let host = Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, security_context.clone()));

    let mut evaluator = Evaluator::with_environment(
        module_registry,
        stdlib_env,
        security_context,
        host,
    );

    evaluator.context_manager.borrow_mut().initialize(Some("root".to_string()));
    evaluator.host.set_execution_context("plan1".to_string(), vec!["intent1".to_string()], "root".to_string());

    use rtfs_compiler::ast::{Expression, Literal, Symbol, Keyword, MapKey, DoExpr, TopLevel};

    // outer step params {"x" "outer"}
    let mut outer_params: std::collections::HashMap<MapKey, Expression> = std::collections::HashMap::new();
    outer_params.insert(MapKey::String("x".to_string()), Expression::Literal(Literal::String("outer".to_string())));

    // inner step params {"x" "inner"}
    let mut inner_params: std::collections::HashMap<MapKey, Expression> = std::collections::HashMap::new();
    inner_params.insert(MapKey::String("x".to_string()), Expression::Literal(Literal::String("inner".to_string())));

    // inner step body returns (get %params "x")
    let inner_step = Expression::List(vec![
        Expression::Symbol(Symbol("step".to_string())),
        Expression::Literal(Literal::String("inner".to_string())),
        Expression::Literal(Literal::Keyword(Keyword("params".to_string()))),
        Expression::Map(inner_params),
        Expression::List(vec![Expression::Symbol(Symbol("get".to_string())), Expression::Symbol(Symbol("%params".to_string())), Expression::Literal(Literal::String("x".to_string()))]),
    ]);

    // outer step body evaluates inner step then returns (get %params "x") from outer
    let outer_step = Expression::List(vec![
        Expression::Symbol(Symbol("step".to_string())),
        Expression::Literal(Literal::String("outer".to_string())),
        Expression::Literal(Literal::Keyword(Keyword("params".to_string()))),
        Expression::Map(outer_params),
        inner_step,
        Expression::List(vec![Expression::Symbol(Symbol("get".to_string())), Expression::Symbol(Symbol("%params".to_string())), Expression::Literal(Literal::String("x".to_string()))]),
    ]);

    let do_expr = Expression::Do(DoExpr { expressions: vec![outer_step] });
    let program = vec![TopLevel::Expression(do_expr)];

    let result = evaluator.eval_toplevel(&program).map_err(|e| format!("eval error: {:?}", e))?;
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(rtfs_compiler::runtime::Value::String(s)) => {
            assert_eq!(s, "outer", "outer %params should be visible after inner step returned");
            Ok(())
        }
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(other) => Err(format!("unexpected result: {:?}", other)),
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => Err("Unexpected host call in pure test".to_string()),
    }
}
