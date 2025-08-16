use rtfs_compiler::runtime::ir_runtime::IrRuntime;
use rtfs_compiler::ir::core::{IrNode, IrMapEntry};
use rtfs_compiler::ir::core::IrNode::*;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use std::sync::Arc;

// Simple smoke test for :params handling in IR Step nodes.
#[test]
fn step_params_bind_as_percent_params() {
    let delegation_engine = Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()));
    let mut runtime = IrRuntime::new_compat(delegation_engine);

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
    let v = res.unwrap();
    // Expect the result to be Integer(123) when reading %params :k
    match v {
        Value::Integer(n) => assert_eq!(n, 123),
        other => panic!("Unexpected result: {:?}", other),
    }
}
