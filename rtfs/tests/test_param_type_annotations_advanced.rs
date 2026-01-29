#![allow(unused_mut)]

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
fn test_variadic_param_annotation_enforced() {
    // Good: rest args are strings
    let code_ok = r#"(let [f (fn [x : Int & rest : :string] : Int x)] (f 1 "a" "b"))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(
        ast_ok.is_ok(),
        "AST should accept variadic type match: {:?}",
        ast_ok
    );
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(1)) => {}
        other => panic!("Expected 1, got {:?}", other),
    }
    assert!(
        ir_ok.is_ok(),
        "IR should accept variadic type match: {:?}",
        ir_ok
    );
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(1)) => {}
        other => panic!("Expected 1, got {:?}", other),
    }

    // Bad: one rest arg not a string
    let code_bad = r#"(let [f (fn [x : Int & rest : :string] : Int x)] (f 1 "a" 2))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should reject non-string variadic arg: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject non-string variadic arg: {:?}",
        ir_bad
    );
}

#[test]
fn test_vector_param_type_annotation() {
    // Param annotated as vector of ints
    let code_ok = r#"(let [sum (fn [v : [:vector :int]] : Int (reduce + v))] (sum [1 2 3]))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(
        ast_ok.is_ok(),
        "AST should accept vector<int>: {:?}",
        ast_ok
    );
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(6)) => {}
        other => panic!("Expected 6, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should accept vector<int>: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(6)) => {}
        other => panic!("Expected 6, got {:?}", other),
    }

    let code_bad = r#"(let [sum (fn [v : [:vector :int]] : Int (reduce + v))] (sum [1 "x"]))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should reject mixed vector: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject mixed vector: {:?}",
        ir_bad
    );
}

#[test]
fn test_tuple_and_destructuring_param_annotation() {
    // Destructure a pair with tuple type annotation
    let code_ok =
        r#"(let [first-of (fn [[a b] : [:tuple :int :string]] : Int a)] (first-of [10 "ok"]))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(ast_ok.is_ok(), "AST should accept tuple arg: {:?}", ast_ok);
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(10)) => {}
        other => panic!("Expected 10, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should accept tuple arg: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(10)) => {}
        other => panic!("Expected 10, got {:?}", other),
    }

    // Wrong shape
    let code_bad_shape =
        r#"(let [first-of (fn [[a b] : [:tuple :int :string]] : Int a)] (first-of [10 20]))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad_shape);
    assert!(
        ast_bad.is_err(),
        "AST should reject wrong tuple shape: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject wrong tuple shape: {:?}",
        ir_bad
    );
}

#[test]
fn test_union_and_optional_param_annotations() {
    // Union: int or string
    let code_int = r#"(let [to-s (fn [x : [:union :int :string]] : :string (str x))] (to-s 5))"#;
    let (ast1, ir1) = eval_ast_and_ir(code_int);
    assert!(ast1.is_ok(), "AST should accept union (int): {:?}", ast1);
    assert!(ir1.is_ok(), "IR should accept union (int): {:?}", ir1);

    let code_str = r#"(let [to-s (fn [x : [:union :int :string]] : :string (str x))] (to-s "ok"))"#;
    let (ast2, ir2) = eval_ast_and_ir(code_str);
    assert!(ast2.is_ok(), "AST should accept union (string): {:?}", ast2);
    assert!(ir2.is_ok(), "IR should accept union (string): {:?}", ir2);

    let code_bad = r#"(let [to-s (fn [x : [:union :int :string]] : :string (str x))] (to-s true))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should reject union mismatch: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject union mismatch: {:?}",
        ir_bad
    );

    // Optional string param accepts nil (use explicit union to avoid sugar parsing differences)
    let code_opt_nil = r#"(let [g (fn [x : [:union :string :nil]] : :string (str x))] (g nil))"#;
    let (ast3, ir3) = eval_ast_and_ir(code_opt_nil);
    assert!(ast3.is_ok(), "AST should accept optional nil: {:?}", ast3);
    assert!(ir3.is_ok(), "IR should accept optional nil: {:?}", ir3);
}

#[test]
fn test_map_param_type_annotation() {
    // Map with required :a:int and optional :b:(string or nil)
    let code_ok = r#"(let [f (fn [{:a a :b b} : [:map [:a :int] [:b [:union :string :nil]]]] : Int a)] (f {:a 2 :b "x"}))"#;
    let (ast_ok, ir_ok) = eval_ast_and_ir(code_ok);
    assert!(ast_ok.is_ok(), "AST should accept typed map: {:?}", ast_ok);
    match ast_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(2)) => {}
        other => panic!("Expected 2, got {:?}", other),
    }
    assert!(ir_ok.is_ok(), "IR should accept typed map: {:?}", ir_ok);
    match ir_ok.unwrap() {
        ExecutionOutcome::Complete(Value::Integer(2)) => {}
        other => panic!("Expected 2, got {:?}", other),
    }

    let code_bad = r#"(let [f (fn [{:a a :b b} : [:map [:a :int] [:b [:union :string :nil]]]] : Int a)] (f {:a "oops"}))"#;
    let (ast_bad, ir_bad) = eval_ast_and_ir(code_bad);
    assert!(
        ast_bad.is_err(),
        "AST should reject wrong field type: {:?}",
        ast_bad
    );
    assert!(
        ir_bad.is_err(),
        "IR should reject wrong field type: {:?}",
        ir_bad
    );
}
