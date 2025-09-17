use rtfs_compiler::runtime::ir_runtime::IrRuntime;
use rtfs_compiler::ir::core::{IrNode, IrMapEntry};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

// Helper function to create test runtime components
fn create_test_runtime() -> (Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface>, RuntimeContext) {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let security_context = RuntimeContext::pure();
    let host = Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    (host, security_context)
}

// 1) Confirm %params binding behavior (duplicate of main test style)
#[test]
fn step_params_success_smoke() {
    // Set fallback context for tests
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "true");
    let (host, security_context) = create_test_runtime();
    let mut runtime = IrRuntime::new(host, security_context);

    // params: {:k 7}
    let map_node = IrNode::Map {
        id: 101,
        entries: vec![IrMapEntry { key: IrNode::Literal { id: 102, value: rtfs_compiler::ast::Literal::Keyword(rtfs_compiler::ast::Keyword::new("k")), ir_type: rtfs_compiler::ir::core::IrType::Keyword, source_location: None }, value: IrNode::Literal { id: 103, value: rtfs_compiler::ast::Literal::Integer(7), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None } }],
        ir_type: rtfs_compiler::ir::core::IrType::Map { entries: vec![], wildcard: None },
        source_location: None,
    };

    let body = vec![IrNode::Apply {
        id: 110,
        function: Box::new(IrNode::Literal { id: 111, value: rtfs_compiler::ast::Literal::Keyword(rtfs_compiler::ast::Keyword::new("k")), ir_type: rtfs_compiler::ir::core::IrType::Keyword, source_location: None }),
        arguments: vec![IrNode::VariableRef { id: 112, name: "%params".to_string(), binding_id: 0, ir_type: rtfs_compiler::ir::core::IrType::Map { entries: vec![], wildcard: None }, source_location: None }],
        ir_type: rtfs_compiler::ir::core::IrType::Any,
        source_location: None,
    }];

    let step = IrNode::Step { id: 120, name: "s1".to_string(), expose_override: None, context_keys_override: None, params: Some(Box::new(map_node)), body, ir_type: rtfs_compiler::ir::core::IrType::Any, source_location: None };
    let program = IrNode::Program { id: 200, version: "1.0".to_string(), forms: vec![step], source_location: None };
    let mut module_registry = rtfs_compiler::runtime::module_runtime::ModuleRegistry::new();
    let outcome = runtime.execute_program(&program, &mut module_registry).expect("program failed");
    let res = match outcome {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    match res { Value::Integer(n) => assert_eq!(n, 7), other => panic!("unexpected: {:?}", other) }
}

// 2) When params evaluation fails, the runtime should notify failure and exit the step context.
// We simulate failure by using a map whose key expression evaluates to a non-string/non-keyword (e.g., a vector)
#[test]
fn step_params_eval_failure_cleanup() {
    let (host, security_context) = create_test_runtime();
    let mut runtime = IrRuntime::new(host, security_context);

    // Build a map with a key that evaluates to a vector (invalid map key)
    let bad_key = IrNode::Vector { id: 201, elements: vec![IrNode::Literal { id: 202, value: rtfs_compiler::ast::Literal::Integer(1), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None }], ir_type: rtfs_compiler::ir::core::IrType::Vector(Box::new(rtfs_compiler::ir::core::IrType::Int)), source_location: None };

    let map_node = IrNode::Map {
        id: 203,
        entries: vec![IrMapEntry { key: bad_key, value: IrNode::Literal { id: 204, value: rtfs_compiler::ast::Literal::Integer(9), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None } }],
        ir_type: rtfs_compiler::ir::core::IrType::Map { entries: vec![], wildcard: None },
        source_location: None,
    };

    let body = vec![IrNode::Literal { id: 210, value: rtfs_compiler::ast::Literal::Integer(0), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None }];

    let step = IrNode::Step { id: 220, name: "s_bad".to_string(), expose_override: None, context_keys_override: None, params: Some(Box::new(map_node)), body, ir_type: rtfs_compiler::ir::core::IrType::Any, source_location: None };
    let program = IrNode::Program { id: 300, version: "1.0".to_string(), forms: vec![step], source_location: None };
    let mut module_registry = rtfs_compiler::runtime::module_runtime::ModuleRegistry::new();

    let res = runtime.execute_program(&program, &mut module_registry);
    // Expect an error indicating invalid params key
    assert!(res.is_err());
}

// 3) If no :params provided, the step should execute body in the same env and return its value.
#[test]
fn step_no_params_executes_body() {
    // Set fallback context for tests
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "true");
    let (host, security_context) = create_test_runtime();
    let mut runtime = IrRuntime::new(host, security_context);

    let body = vec![IrNode::Literal { id: 310, value: rtfs_compiler::ast::Literal::Integer(55), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None }];
    let step = IrNode::Step { id: 320, name: "s_no_params".to_string(), expose_override: None, context_keys_override: None, params: None, body, ir_type: rtfs_compiler::ir::core::IrType::Any, source_location: None };
    let program = IrNode::Program { id: 400, version: "1.0".to_string(), forms: vec![step], source_location: None };
    let mut module_registry = rtfs_compiler::runtime::module_runtime::ModuleRegistry::new();
    let outcome = runtime.execute_program(&program, &mut module_registry).expect("program failed");
    let res = match outcome {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    match res { Value::Integer(n) => assert_eq!(n, 55), other => panic!("unexpected: {:?}", other) }
}
