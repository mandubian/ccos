use rtfs::ir::core::{IrMapEntry, IrNode};
use rtfs::runtime::ir_runtime::IrRuntime;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use std::sync::Arc;

// Helper function to create test runtime components
fn create_test_runtime() -> (
    Arc<dyn rtfs::runtime::host_interface::HostInterface>,
    RuntimeContext,
) {
    let security_context = RuntimeContext::pure();
    let host = create_pure_host();
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
        entries: vec![IrMapEntry {
            key: IrNode::Literal {
                id: 2,
                value: rtfs::ast::Literal::Keyword(rtfs::ast::Keyword::new("k")),
                ir_type: rtfs::ir::core::IrType::Keyword,
                source_location: None,
            },
            value: IrNode::Literal {
                id: 3,
                value: rtfs::ast::Literal::Integer(123),
                ir_type: rtfs::ir::core::IrType::Int,
                source_location: None,
            },
        }],
        ir_type: rtfs::ir::core::IrType::Map {
            entries: vec![],
            wildcard: None,
        },
        source_location: None,
    };

    // Body reads %params :k using keyword lookup (:k %params) equivalent to get
    let body = vec![IrNode::Apply {
        id: 10,
        // Use a Keyword literal for :k so apply treats it as a keyword lookup on the map argument.
        function: Box::new(IrNode::Literal {
            id: 11,
            value: rtfs::ast::Literal::Keyword(rtfs::ast::Keyword::new("k")),
            ir_type: rtfs::ir::core::IrType::Keyword,
            source_location: None,
        }),
        arguments: vec![IrNode::VariableRef {
            id: 12,
            name: "%params".to_string(),
            binding_id: 0,
            ir_type: rtfs::ir::core::IrType::Map {
                entries: vec![],
                wildcard: None,
            },
            source_location: None,
        }],
        ir_type: rtfs::ir::core::IrType::Any,
        source_location: None,
    }];

    let step = IrNode::Step {
        id: 20,
        name: "s".to_string(),
        expose_override: None,
        context_keys_override: None,
        params: Some(Box::new(map_node)),
        body,
        ir_type: rtfs::ir::core::IrType::Any,
        source_location: None,
    };

    let mut module_registry = rtfs::runtime::module_runtime::ModuleRegistry::new();
    // Execute the step as a program
    let program = IrNode::Program {
        id: 100,
        version: "1.0".to_string(),
        forms: vec![step],
        source_location: None,
    };

    let res = runtime.execute_program(&program, &mut module_registry);
    if let Err(e) = &res {
        eprintln!("Runtime execute_program returned error: {:?}", e);
    }
    assert!(res.is_ok());
    let outcome = res.unwrap();
    let v = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    // Expect the result to be Integer(123) when reading %params :k
    match v {
        Value::Integer(n) => assert_eq!(n, 123),
        other => panic!("Unexpected result: {:?}", other),
    }
}
