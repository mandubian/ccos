use std::rc::Rc;
use std::sync::Arc;

use rtfs_compiler::bytecode::WasmBackend;
use rtfs_compiler::ccos::caching::l4_content_addressable::{L4CacheClient, RtfsModuleMetadata};
use rtfs_compiler::ccos::delegation::{DelegationEngine, StaticDelegationEngine};
use rtfs_compiler::ccos::delegation_l4::L4AwareDelegationEngine;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry, Value, RuntimeResult};
use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::runtime::values::{Function};
use rtfs_compiler::ast::{Literal, Symbol, Expression};
use wat::parse_str;
use rtfs_compiler::parser::parse_expression;

#[test]
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
    let mut evaluator = Evaluator::new(Rc::new(module_registry), de);
    let symbol_add = Symbol("add".to_string());
    // Create a dummy closure that won't actually be executed when delegation takes L4 path.
    let dummy_closure = Function::new_closure(
        vec![Symbol("x".to_string()), Symbol("y".to_string())],
        Box::new(rtfs_compiler::ast::Expression::Literal(Literal::Nil)),
        Rc::new(Environment::new()),
        None,
    );
    evaluator.env.define(&symbol_add, Value::Function(dummy_closure));

    // 6. Parse and evaluate the RTFS expression
    let code = "(add 1 2)";
    let expr = parse_expression(code).expect("failed to parse expression");
    let result = evaluator.evaluate(&expr)?;
    assert_eq!(result, Value::Integer(3));
    Ok(())
}

#[test]
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
    let de: Arc<dyn DelegationEngine> = Arc::new(L4AwareDelegationEngine::new(cache.clone(), inner));

    let mut evaluator = Evaluator::new(Rc::new(module_registry), de);

    let code = "(do (defn add [x y] nil) (add 1 2))";
    let expr = parse_expression(code).unwrap();
    let result = evaluator.evaluate(&expr)?;
    assert_eq!(result, Value::Integer(3));
    Ok(())
} 