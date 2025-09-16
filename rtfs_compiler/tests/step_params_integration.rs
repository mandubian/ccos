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

// This test exercises the (step ...) special form with :params and ensures
// bound parameters are available to the step body via the reserved symbol %params.

#[test]
fn step_params_binding_visible_in_body() -> Result<(), String> {
    // Build evaluator like test helpers would do but using only public APIs
    let module_registry = Arc::new(ModuleRegistry::new());
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

    // Ensure the evaluator has a root execution context so enter_step can create children
    evaluator.context_manager.borrow_mut().initialize(Some("root".to_string()));
    // Set a minimal execution context in the host so notify_step_started/completed can record actions
    evaluator.host.set_execution_context("plan1".to_string(), vec!["intent1".to_string()], "root".to_string());

    // Construct the AST directly to avoid depending on parser details in this unit test.
    use rtfs_compiler::ast::{Expression, Literal, Symbol, Keyword, MapKey, DoExpr, TopLevel};

    // Build the :params map {"a" 1 "b" "x"}
    let mut params_map: std::collections::HashMap<MapKey, Expression> = std::collections::HashMap::new();
    params_map.insert(MapKey::String("a".to_string()), Expression::Literal(Literal::Integer(1)));
    params_map.insert(MapKey::String("b".to_string()), Expression::Literal(Literal::String("x".to_string())));

    // Build the (get %params "a") and (get %params "b") expressions as lists
    let get_a = Expression::List(vec![
        Expression::Symbol(Symbol("get".to_string())),
        Expression::Symbol(Symbol("%params".to_string())),
        Expression::Literal(Literal::String("a".to_string())),
    ]);
    let get_b = Expression::List(vec![
        Expression::Symbol(Symbol("get".to_string())),
        Expression::Symbol(Symbol("%params".to_string())),
        Expression::Literal(Literal::String("b".to_string())),
    ]);

    // Build the step expression as a list: (step "s1" :params { ... } <body...>)
    let step_expr = Expression::List(vec![
        Expression::Symbol(Symbol("step".to_string())),
        Expression::Literal(Literal::String("s1".to_string())),
        Expression::Literal(Literal::Keyword(Keyword("params".to_string()))),
        Expression::Map(params_map),
        get_a,
        get_b,
    ]);

    // Wrap in a do expression
    let do_expr = Expression::Do(DoExpr { expressions: vec![step_expr] });
    let program = vec![TopLevel::Expression(do_expr)];

    let result = evaluator.eval_toplevel(&program).map_err(|e| format!("eval error: {:?}", e))?;

    match result {
        rtfs_compiler::runtime::Value::String(s) => {
            if s == "x" { Ok(()) } else { Err(format!("unexpected result string: {}", s)) }
        }
        other => Err(format!("unexpected result type: {:?}", other)),
    }
}
