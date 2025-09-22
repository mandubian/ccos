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

// Simple smoke test for :params handling in IR Step nodes.
#[test]
fn step_params_bind_as_percent_params() {
    // Set fallback context for tests
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "true");
    let (host, security_context) = create_test_runtime();
    let mut runtime = IrRuntime::new(host, security_context);

    // Construct a params map: {"k": 123}
    let map_node = IrNode::Map {
        id: 1,
        entries: vec![IrMapEntry { key: IrNode::Literal { id: 2, value: rtfs_compiler::ast::Literal::Keyword(rtfs_compiler::ast::Keyword::new("k")), ir_type: rtfs_compiler::ir::core::IrType::Keyword, source_location: None }, value: IrNode::Literal { id: 3, value: rtfs_compiler::ast::Literal::Integer(123), ir_type: rtfs_compiler::ir::core::IrType::Int, source_location: None } }],
        ir_type: rtfs_compiler::ir::core::IrType::Map { entries: vec![], wildcard: None },
        source_location: None,
    };

    // Body reads %params :k using keyword lookup (:k %params) equivalent to get
    let body = vec![IrNode::Apply {
        id: 10,
        // Use a Keyword literal for :k so apply treats it as a keyword lookup on the map argument.
        function: Box::new(IrNode::Literal { id: 11, value: rtfs_compiler::ast::Literal::Keyword(rtfs_compiler::ast::Keyword::new("k")), ir_type: rtfs_compiler::ir::core::IrType::Keyword, source_location: None }),
        arguments: vec![IrNode::VariableRef { id: 12, name: "%params".to_string(), binding_id: 0, ir_type: rtfs_compiler::ir::core::IrType::Map { entries: vec![], wildcard: None }, source_location: None }],
        ir_type: rtfs_compiler::ir::core::IrType::Any,
        source_location: None,
    }];

    let step = IrNode::Step {
        id: 20,
        name: "s".to_string(),
        expose_override: None,
        context_keys_override: None,
        params: Some(Box::new(map_node)),
        body,
        ir_type: rtfs_compiler::ir::core::IrType::Any,
        source_location: None,
    };

    let mut module_registry = rtfs_compiler::runtime::module_runtime::ModuleRegistry::new();
    // Execute the step as a program
    let program = IrNode::Program { id: 100, version: "1.0".to_string(), forms: vec![step], source_location: None };

    let res = runtime.execute_program(&program, &mut module_registry);
    if let Err(e) = &res {
        eprintln!("Runtime execute_program returned error: {:?}", e);
    }
    assert!(res.is_ok());
    let outcome = res.unwrap();
    let v = match outcome {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    // Expect the result to be Integer(123) when reading %params :k
    match v {
        Value::Integer(n) => assert_eq!(n, 123),
        other => panic!("Unexpected result: {:?}", other),
    }
}
