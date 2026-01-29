use rtfs::parser;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::execution_outcome::ExecutionOutcome;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::RuntimeStrategy; // bring .run() into scope for IR strategy
use std::sync::Arc;

fn eval_ast_and_ir(
    code: &str,
) -> (
    Result<ExecutionOutcome, String>,
    Result<ExecutionOutcome, String>,
) {
    // Parse to AST
    let parsed = match parser::parse_expression(code) {
        Ok(ast) => ast,
        Err(e) => {
            return (
                Err(format!("Parse error: {:?}", e)),
                Err("parse failed".into()),
            )
        }
    };

    // AST evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let ast_res = evaluator
        .evaluate(&parsed)
        .map_err(|e| format!("AST evaluation error: {}", e));

    // IR runtime (via strategy that converts internally)
    let module_registry = Arc::new(ModuleRegistry::new());
    let mut ir_strategy = rtfs::runtime::ir_runtime::IrStrategy::new(module_registry);
    let ir_res = ir_strategy
        .run(&parsed)
        .map_err(|e| format!("IR runtime error: {}", e));

    (ast_res, ir_res)
}

#[test]
fn test_fn_param_annotation_accepts_int_symbol_and_keyword() {
    // Symbol form (Int)
    let code_symbol = r#"(let [id (fn [x : Int] x)] (id 42))"#;
    let (ast1, ir1) = eval_ast_and_ir(code_symbol);
    assert!(ast1.is_ok(), "AST should succeed: {:?}", ast1);
    match ast1.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(42)) => {}
        other => panic!("Expected 42, got {:?}", other),
    }
    assert!(ir1.is_ok(), "IR should succeed: {:?}", ir1);
    match ir1.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(42)) => {}
        other => panic!("Expected 42, got {:?}", other),
    }

    // Keyword primitive form (:int)
    let code_keyword = r#"(let [id (fn [x : :int] x)] (id 7))"#;
    let (ast2, ir2) = eval_ast_and_ir(code_keyword);
    assert!(ast2.is_ok(), "AST should succeed: {:?}", ast2);
    match ast2.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(7)) => {}
        other => panic!("Expected 7, got {:?}", other),
    }
    assert!(ir2.is_ok(), "IR should succeed: {:?}", ir2);
    match ir2.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(7)) => {}
        other => panic!("Expected 7, got {:?}", other),
    }
}

#[test]
fn test_fn_param_annotation_mismatch_rejected() {
    // Annotated as Int but pass a Float
    let code = r#"(let [id (fn [x : Int] x)] (id 1.5))"#;
    let (ast, ir) = eval_ast_and_ir(code);

    assert!(ast.is_err(), "AST should fail type check: {:?}", ast);
    assert!(ir.is_err(), "IR should fail type check: {:?}", ir);
}

#[test]
fn test_fn_return_type_annotation_enforced() {
    // Declares return : Int but returns a string -> should error
    let code_bad = r#"(let [f (fn [x : Int] : Int (str x))] (f 1))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should reject mismatched return type: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject mismatched return type: {:?}",
        ir_bad
    );

    // Declares return : Int and returns (+ x 1) -> ok
    let code_ok = r#"(let [f (fn [x : Int] : Int (+ x 1))] (f 1))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(
        ast_ok.is_ok(),
        "AST should accept correct return type: {:?}",
        ast_ok
    );
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(2)) => {}
        other => panic!("Expected 2, got {:?}", other),
    }
    assert!(
        ir_ok.is_ok(),
        "IR should accept correct return type: {:?}",
        ir_ok
    );
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(2)) => {}
        other => panic!("Expected 2, got {:?}", other),
    }
}
