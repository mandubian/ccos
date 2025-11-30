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
fn test_hof_param_function_enforced_on_apply() {
    // Good inner function: param/return annotated; outer applies without annotating f
    let code_ok = r#"(let [applyF (fn [f x] (f x))
                    good   (fn [y : Int] : Int (+ y 1))]
            (applyF good 2))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(ast_ok.is_ok(), "AST should succeed: {:?}", ast_ok);
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(3)) => {}
        other => panic!("Expected 3, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should succeed: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(3)) => {}
        other => panic!("Expected 3, got {:?}", other),
    }

    // Bad inner function: param annotated as String, but outer passes an Int -> should fail at inner call
    let code_bad = r#"(let [applyF (fn [f x] (f x))
                    bad    (fn [y : String] : Int 1)]
            (applyF bad 2))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should fail inner param type: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should fail inner param type: {:?}",
        ir_bad
    );
}

#[test]
fn test_hof_variadic_inner_enforced() {
    // Outer applies inner with an int then multiple string args; inner has fixed int + variadic string rest
    let code_ok = r#"(let [apply2 (fn [f] (f 1 "a" "b"))
                    ok     (fn [x : Int & rest : :string] : Int 42)]
            (apply2 ok))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(ast_ok.is_ok(), "AST should succeed: {:?}", ast_ok);
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(42)) => {}
        other => panic!("Expected 42, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should succeed: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(42)) => {}
        other => panic!("Expected 42, got {:?}", other),
    }

    // Mismatch: inner expects variadic Ints but receives strings after the fixed Int
    let code_bad = r#"(let [apply2 (fn [f] (f 1 "a"))
                    bad    (fn [x : Int & rest : :int] : Int 0)]
            (apply2 bad))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should fail variadic rest type: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should fail variadic rest type: {:?}",
        ir_bad
    );
}

#[test]
fn test_hof_return_function_applied_later_enforced() {
    // Return a function; no structural function type checking is performed at return boundary,
    // but inner function's own annotations are enforced when called later.
    let code_ok = r#"(let [make-adder (fn [n : Int] : :any (fn [m : Int] : Int (+ n m)))
                    adder      (make-adder 5)]
            (adder 3))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(ast_ok.is_ok(), "AST should succeed: {:?}", ast_ok);
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(8)) => {}
        other => panic!("Expected 8, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should succeed: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(8)) => {}
        other => panic!("Expected 8, got {:?}", other),
    }

    // Calling returned fn with wrong type should be rejected by the inner fn's annotations
    let code_bad = r#"(let [make-adder (fn [n : Int] : :any (fn [m : Int] : Int (+ n m)))
                    adder      (make-adder 5)]
            (adder "x"))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should fail inner param type on returned fn: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should fail inner param type on returned fn: {:?}",
        ir_bad
    );
}

#[test]
fn test_hof_function_type_annotation_not_yet_supported() {
    // Known gap: structural validation of function types is not implemented.
    // Annotating a parameter as a function type should currently be rejected by the validator.
    let code = r#"
    (let [applyF (fn [f : [:fn [:int] :int] x : Int] : Int (f x))
          good   (fn [y : Int] : Int (+ y 1))]
      (applyF good 2))
    "#;
    let (ast_res, ir_res) = eval_ast_and_ir(code);
    assert!(
        ast_res.is_err(),
        "AST should currently reject function type annotations for values: {:?}",
        ast_res
    );
    assert!(
        ir_res.is_err(),
        "IR should currently reject function type annotations for values: {:?}",
        ir_res
    );
}
