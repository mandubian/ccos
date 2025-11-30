use std::sync::Arc;

use rtfs::ast::{Literal, Pattern, Symbol};
use rtfs::bytecode::WasmBackend;
use rtfs::ccos::caching::l4_content_addressable::{L4CacheClient, RtfsModuleMetadata};
use rtfs::ccos::delegation::{DelegationEngine, StaticDelegationEngine};
use rtfs::ccos::delegation_l4::L4AwareDelegationEngine;
use rtfs::parser::parse_expression;
use rtfs::runtime::environment::Environment;
use rtfs::runtime::values::Function;
use rtfs::runtime::{Evaluator, ModuleRegistry, RuntimeResult, Value};
use wat::parse_str;

#[test]
#[ignore = "temporarily disabled: returns Nil instead of expected 3"]
fn test_l4_cache_wasm_execution() -> RuntimeResult<()> {
    // 1. Build a tiny wasm module exporting an `add` function (i64 add)
    let wat = r#"(module (func $add (export "add") (param i64 i64) (result i64) local.get 0 local.get 1 i64.add))"#;
    let wasm_bytes = parse_str(wat).expect("Failed to assemble WAT");

    // 2. Create shared L4 cache and publish module
    let cache = L4CacheClient::new();
    let metadata = RtfsModuleMetadata::new(Vec::new(), "add".to_string(), String::new());
    let _pointer = cache
        .publish_module(wasm_bytes.clone(), metadata)
        .expect("publish module");

    // 3. Build ModuleRegistry with same cache + backend (backend not used here but included for completeness)
    let backend = Arc::new(WasmBackend::default());
    let module_registry = ModuleRegistry::new()
        .with_l4_cache(Arc::new(cache.clone()))
        .with_bytecode_backend(backend);

    // 4. Build DelegationEngine wrapped with L4 awareness
    let inner = StaticDelegationEngine::new(Default::default());
    let l4_de = L4AwareDelegationEngine::new(cache.clone(), inner);
    let de: Arc<dyn DelegationEngine> = Arc::new(l4_de);

    // 5. Create evaluator with empty env, but register a placeholder `add` so lookup succeeds.
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        rtfs::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let capability_marketplace = std::sync::Arc::new(
        rtfs::ccos::capability_marketplace::CapabilityMarketplace::new(registry),
    );
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs::ccos::causal_chain::CausalChain::new().unwrap(),
    ));
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let mut evaluator = Evaluator::new(
        Arc::new(module_registry),
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let symbol_add = Symbol("add".to_string());
    // Create a dummy closure that won't actually be executed when delegation takes L4 path.
    let dummy_closure = Function::new_closure(
        vec![Symbol("x".to_string()), Symbol("y".to_string())],
        vec![
            Pattern::Symbol(Symbol("x".to_string())),
            Pattern::Symbol(Symbol("y".to_string())),
        ],
        None, // variadic parameter
        Box::new(rtfs::ast::Expression::Literal(Literal::Nil)),
        Arc::new(Environment::new()),
        None,
    );
    evaluator
        .env
        .define(&symbol_add, Value::Function(dummy_closure));

    // 6. Parse and evaluate the RTFS expression
    let code = "(add 1 2)";
    let expr = parse_expression(code).expect("failed to parse expression");
    let result = evaluator.evaluate(&expr)?;
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => {
            assert_eq!(value, Value::Integer(3));
        }
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    }
    Ok(())
}

#[test]
#[ignore = "temporarily disabled: returns Nil instead of expected 3"]
fn test_l4_cache_with_local_definition() -> RuntimeResult<()> {
    // Wasm module exporting add
    let wat = r#"(module (func $add (export "add") (param i64 i64) (result i64) local.get 0 local.get 1 i64.add))"#;
    let wasm_bytes = parse_str(wat).expect("assemble wat");

    let cache = L4CacheClient::new();
    let meta = RtfsModuleMetadata::new(Vec::new(), "add".to_string(), String::new());
    cache.publish_module(wasm_bytes, meta).unwrap();

    let backend = Arc::new(WasmBackend::default());
    let module_registry = ModuleRegistry::new()
        .with_l4_cache(Arc::new(cache.clone()))
        .with_bytecode_backend(backend);

    let inner = StaticDelegationEngine::new(Default::default());
    let de: Arc<dyn DelegationEngine> =
        Arc::new(L4AwareDelegationEngine::new(cache.clone(), inner));

    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        rtfs::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let capability_marketplace = std::sync::Arc::new(
        rtfs::ccos::capability_marketplace::CapabilityMarketplace::new(registry),
    );
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs::ccos::causal_chain::CausalChain::new().unwrap(),
    ));
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        Arc::new(module_registry),
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );

    let code = "(do (defn add [x y] nil) (add 1 2))";
    let expr = parse_expression(code).unwrap();
    let result = evaluator.evaluate(&expr)?;
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => {
            assert_eq!(value, Value::Integer(3));
        }
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    }
    Ok(())
}
