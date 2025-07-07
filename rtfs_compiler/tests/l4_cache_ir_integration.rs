use std::sync::Arc;
use std::rc::Rc;

use bincode;
use rtfs_compiler::{
    ccos::{
        caching::l4_content_addressable::L4CacheClient,
        delegation::{DelegationEngine, StaticDelegationEngine},
    },
    ir::{core::IrNode, converter::IrConverter},
    parser::parse_expression,
    runtime::{
        ir_runtime::IrRuntime,
        environment::IrEnvironment,
        module_runtime::ModuleRegistry,
        Value,
        values::{Function, BuiltinFunction, Arity},
        error::RuntimeResult,
    },
    ast::Literal,
    ir::core::IrType,
};

#[test]
fn test_ir_cached_execution() {
    // Build IR for a simple anonymous add function
    let src_fn = "(fn [x y] (+ x y))";
    let ast_fn = parse_expression(src_fn).expect("parse fn expr");

    let module_registry = ModuleRegistry::new();
    let mut converter = IrConverter::with_module_registry(&module_registry);
    let ir_lambda = converter
        .convert_expression(ast_fn)
        .expect("convert to IR");

    // Serialize IR and store in cache
    let bytes = bincode::serialize(&ir_lambda).expect("serialize");
    let cache = L4CacheClient::new();
    let hash = cache.store_blob(bytes).expect("store blob");

    // Retrieve and deserialize
    let stored = cache.get_blob(&hash).expect("retrieve");
    let deserialized: IrNode = bincode::deserialize(&stored).expect("deserialize");

    // Build Apply node: (lambda 4 5)
    let lit4 = IrNode::Literal {
        id: 1001,
        value: Literal::Integer(4),
        ir_type: IrType::Int,
        source_location: None,
    };
    let lit5 = IrNode::Literal {
        id: 1002,
        value: Literal::Integer(5),
        ir_type: IrType::Int,
        source_location: None,
    };
    let apply_node = IrNode::Apply {
        id: 1003,
        function: Box::new(deserialized),
        arguments: vec![lit4, lit5],
        ir_type: IrType::Int,
        source_location: None,
    };

    // Execute through IR runtime
    let de_inner = StaticDelegationEngine::new(Default::default());
    let de: Arc<dyn DelegationEngine> = Arc::new(de_inner);
    let mut ir_runtime = IrRuntime::new(de);
    let mut env = IrEnvironment::with_stdlib(&module_registry).expect("env");

    // Inject simple builtin '+' into environment
    env.define(
        "+".to_string(),
        Value::Function(Function::Builtin(BuiltinFunction {
            name: "+".to_string(),
            arity: Arity::Variadic(2),
            func: std::rc::Rc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                let sum = args
                    .iter()
                    .map(|v| match v {
                        Value::Integer(n) => *n,
                        _ => 0,
                    })
                    .sum();
                Ok(Value::Integer(sum))
            }),
        })),
    );

    let result = ir_runtime
        .execute_node(&apply_node, &mut env, false, &mut ModuleRegistry::new())
        .expect("run IR");

    assert_eq!(result, Value::Integer(9));
} 