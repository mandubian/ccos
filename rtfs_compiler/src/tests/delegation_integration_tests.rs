use crate::ast::{DelegationHint, Expression, FnExpr, Literal, Pattern, Symbol};
use crate::ccos::delegation::{ExecTarget, LocalEchoModel, ModelRegistry, RemoteArbiterModel, StaticDelegationEngine};
use crate::parser;
use crate::runtime::{Environment, Evaluator, IrRuntime, ModuleRegistry};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn test_delegation_hint_local_model() {
    // Test that delegation hints work correctly with local models
    let mut map = HashMap::new();
    map.insert("test-fn".to_string(), ExecTarget::LocalModel("echo-model".to_string()));
    let delegation_engine = Arc::new(StaticDelegationEngine::new(map));
    
    let module_registry = Rc::new(ModuleRegistry::new());
    let mut evaluator = Evaluator::new(module_registry, delegation_engine);
    
    // Create a function with delegation hint
    let fn_expr = FnExpr {
        params: vec![Pattern::Symbol(Symbol("x".to_string()))],
        body: Expression::Literal(Literal::String("test".to_string())),
        delegation_hint: Some(DelegationHint::LocalModel("echo-model".to_string())),
    };
    
    // This should work - the function will be delegated to the echo model
    let result = evaluator.eval_fn(&fn_expr, &mut Environment::new());
    assert!(result.is_ok());
}

#[test]
fn test_delegation_hint_remote_model() {
    // Test that delegation hints work correctly with remote models
    let mut map = HashMap::new();
    map.insert("test-fn".to_string(), ExecTarget::RemoteModel("arbiter-remote".to_string()));
    let delegation_engine = Arc::new(StaticDelegationEngine::new(map));
    
    let module_registry = Rc::new(ModuleRegistry::new());
    let mut evaluator = Evaluator::new(module_registry, delegation_engine);
    
    // Create a function with delegation hint
    let fn_expr = FnExpr {
        params: vec![Pattern::Symbol(Symbol("x".to_string()))],
        body: Expression::Literal(Literal::String("test".to_string())),
        delegation_hint: Some(DelegationHint::RemoteModel("arbiter-remote".to_string())),
    };
    
    // This should work - the function will be delegated to the remote model
    let result = evaluator.eval_fn(&fn_expr, &mut Environment::new());
    assert!(result.is_ok());
}

#[test]
fn test_model_registry_integration() {
    // Test that the model registry is properly integrated
    let registry = ModelRegistry::with_defaults();
    
    // Verify default providers are available
    assert!(registry.get("echo-model").is_some());
    assert!(registry.get("arbiter-remote").is_some());
    
    // Test echo model functionality
    let echo_provider = registry.get("echo-model").unwrap();
    let result = echo_provider.infer("hello").unwrap();
    assert_eq!(result, "[ECHO] hello");
    
    // Test remote model functionality
    let remote_provider = registry.get("arbiter-remote").unwrap();
    let result = remote_provider.infer("test").unwrap();
    assert!(result.contains("[REMOTE:http://localhost:8080/arbiter]"));
    assert!(result.contains("test"));
}

#[test]
fn test_ir_runtime_delegation() {
    // Test that IR runtime also supports delegation
    let mut map = HashMap::new();
    map.insert("test-fn".to_string(), ExecTarget::LocalModel("echo-model".to_string()));
    let delegation_engine = Arc::new(StaticDelegationEngine::new(map));
    
    let mut ir_runtime = IrRuntime::new_compat(delegation_engine);
    
    // The IR runtime should have the model registry available
    // This test verifies that the delegation engine integration is complete
    assert!(ir_runtime.model_registry.get("echo-model").is_some());
}

#[test]
fn test_delegation_engine_policy() {
    // Test that the delegation engine respects static policies
    let mut map = HashMap::new();
    map.insert("math/add".to_string(), ExecTarget::LocalModel("echo-model".to_string()));
    map.insert("http/get".to_string(), ExecTarget::RemoteModel("arbiter-remote".to_string()));
    
    let delegation_engine = Arc::new(StaticDelegationEngine::new(map));
    
    // Test that the policies are respected
    let ctx = crate::ccos::delegation::CallContext {
        fn_symbol: "math/add",
        arg_type_fingerprint: 0,
        runtime_context_hash: 0,
        semantic_hash: None,
        metadata: None,
    };
    assert_eq!(delegation_engine.decide(&ctx), ExecTarget::LocalModel("echo-model".to_string()));
    
    let ctx = crate::ccos::delegation::CallContext {
        fn_symbol: "http/get",
        arg_type_fingerprint: 0,
        runtime_context_hash: 0,
        semantic_hash: None,
        metadata: None,
    };
    assert_eq!(delegation_engine.decide(&ctx), ExecTarget::RemoteModel("arbiter-remote".to_string()));
    
    // Test fallback for unknown functions
    let ctx = crate::ccos::delegation::CallContext {
        fn_symbol: "unknown/fn",
        arg_type_fingerprint: 0,
        runtime_context_hash: 0,
        semantic_hash: None,
        metadata: None,
    };
    assert_eq!(delegation_engine.decide(&ctx), ExecTarget::LocalPure);
} 