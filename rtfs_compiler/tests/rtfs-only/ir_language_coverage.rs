// Integration tests for IR coverage of all language features
// Tests systematic conversion from AST to IR for every language construct

use rtfs_compiler::ast::*;
use rtfs_compiler::ir::converter::IrConverter;
use rtfs_compiler::ir::core::*;
use std::collections::HashMap;

fn setup_converter() -> IrConverter<'static> {
    IrConverter::new()
}

#[test]
fn test_ir_conversion_literals() {
    let mut converter = setup_converter();

    // Integer literal
    let ast = Expression::Literal(Literal::Integer(42));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert integer");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Integer(42),
            ..
        }
    ));

    // Float literal
    let ast = Expression::Literal(Literal::Float(3.14));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert float");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Float(_),
            ..
        }
    ));

    // String literal
    let ast = Expression::Literal(Literal::String("hello".to_string()));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert string");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::String(_),
            ..
        }
    ));

    // Boolean literal
    let ast = Expression::Literal(Literal::Boolean(true));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert boolean");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Boolean(true),
            ..
        }
    ));

    // Nil literal
    let ast = Expression::Literal(Literal::Nil);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert nil");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Nil,
            ..
        }
    ));

    // Keyword literal
    let ast = Expression::Literal(Literal::Keyword(Keyword::new("test")));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert keyword");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Keyword(_),
            ..
        }
    ));
}

#[test]
fn test_ir_conversion_new_literal_types() {
    let mut converter = setup_converter();

    // Timestamp literal
    let ast = Expression::Literal(Literal::Timestamp("2023-01-01T00:00:00Z".to_string()));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert timestamp");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Timestamp(_),
            ..
        }
    ));

    // UUID literal
    let ast = Expression::Literal(Literal::Uuid(
        "550e8400-e29b-41d4-a716-446655440000".to_string(),
    ));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert UUID");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::Uuid(_),
            ..
        }
    ));

    // Resource handle literal
    let ast = Expression::Literal(Literal::ResourceHandle("resource-123".to_string()));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert resource handle");
    assert!(matches!(
        ir,
        IrNode::Literal {
            value: Literal::ResourceHandle(_),
            ..
        }
    ));
}

#[test]
fn test_ir_conversion_collections() {
    let mut converter = setup_converter();

    // Vector
    let ast = Expression::Vector(vec![
        Expression::Literal(Literal::Integer(1)),
        Expression::Literal(Literal::Integer(2)),
        Expression::Literal(Literal::Integer(3)),
    ]);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert vector");
    assert!(matches!(ir, IrNode::Vector { elements, .. } if elements.len() == 3));

    // Map
    let mut map = HashMap::new();
    map.insert(
        MapKey::Keyword(Keyword::new("name")),
        Expression::Literal(Literal::String("test".to_string())),
    );
    map.insert(
        MapKey::String("count".to_string()),
        Expression::Literal(Literal::Integer(5)),
    );
    let ast = Expression::Map(map);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert map");
    assert!(matches!(ir, IrNode::Map { entries, .. } if entries.len() == 2));
}

#[test]
fn test_ir_conversion_control_flow() {
    let mut converter = setup_converter();

    // If expression
    let if_expr = IfExpr {
        condition: Box::new(Expression::Literal(Literal::Boolean(true))),
        then_branch: Box::new(Expression::Literal(Literal::Integer(1))),
        else_branch: Some(Box::new(Expression::Literal(Literal::Integer(2)))),
    };
    let ast = Expression::If(if_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert if expression");
    assert!(matches!(ir, IrNode::If { else_branch, .. } 
        if else_branch.is_some()));
}

#[test]
fn test_ir_conversion_let_binding() {
    let mut converter = setup_converter();

    let let_expr = LetExpr {
        bindings: vec![LetBinding {
            pattern: Pattern::Symbol(Symbol::new("x")),
            type_annotation: None,
            value: Box::new(Expression::Literal(Literal::Integer(42))),
        }],
        body: vec![Expression::Symbol(Symbol::new("x"))],
    };
    let ast = Expression::Let(let_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert let expression");
    assert!(matches!(ir, IrNode::Let { bindings, body, .. } 
        if bindings.len() == 1 && body.len() == 1));
}

#[test]
fn test_ir_conversion_function_definition() {
    let mut converter = setup_converter();

    let fn_expr = FnExpr {
        params: vec![ParamDef {
            pattern: Pattern::Symbol(Symbol::new("x")),
            type_annotation: None,
        }],
        variadic_param: None,
        body: vec![Expression::Symbol(Symbol::new("x"))],
        return_type: None,
        delegation_hint: None,
    };
    let ast = Expression::Fn(fn_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert function expression");
    assert!(matches!(ir, IrNode::Lambda { params, body, .. } 
        if params.len() == 1 && body.len() == 1));
}

#[test]
fn test_ir_conversion_function_call() {
    let mut converter = setup_converter();

    let ast = Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol::new("+"))),
        arguments: vec![
            Expression::Literal(Literal::Integer(1)),
            Expression::Literal(Literal::Integer(2)),
        ],
    };
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert function call");
    assert!(matches!(ir, IrNode::Apply { arguments, .. } 
        if arguments.len() == 2));
}

#[test]
fn test_ir_conversion_def_and_defn() {
    let mut converter = setup_converter();

    // Def expression
    let def_expr = DefExpr {
        symbol: Symbol::new("my-var"),
        value: Box::new(Expression::Literal(Literal::Integer(42))),
        type_annotation: None,
    };
    let ast = Expression::Def(Box::new(def_expr));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert def expression");
    assert!(matches!(ir, IrNode::VariableDef { name, .. } 
        if name == "my-var"));

    // Defn expression
    let defn_expr = DefnExpr {
        name: Symbol::new("my-func"),
        params: vec![ParamDef {
            pattern: Pattern::Symbol(Symbol::new("x")),
            type_annotation: None,
        }],
        variadic_param: None,
        body: vec![Expression::Symbol(Symbol::new("x"))],
        return_type: None,
        delegation_hint: None,
        metadata: None,
    };
    let ast = Expression::Defn(Box::new(defn_expr));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert defn expression");
    assert!(matches!(ir, IrNode::FunctionDef { name, .. } 
        if name == "my-func"));
}

#[test]
fn test_ir_conversion_do_expression() {
    let mut converter = setup_converter();

    // Do expression
    let do_expr = DoExpr {
        expressions: vec![
            Expression::Literal(Literal::Integer(1)),
            Expression::Literal(Literal::Integer(2)),
        ],
    };
    let ast = Expression::Do(do_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert do expression");
    assert!(matches!(ir, IrNode::Do { expressions, .. } 
        if expressions.len() == 2));
}

#[test]
fn test_ir_conversion_try_catch() {
    let mut converter = setup_converter();

    // TryCatch expression
    let try_expr = TryCatchExpr {
        try_body: vec![Expression::Literal(Literal::Integer(1))],
        catch_clauses: vec![],
        finally_body: None,
    };
    let ast = Expression::TryCatch(try_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert try-catch expression");
    assert!(matches!(ir, IrNode::TryCatch { try_body, .. } 
        if try_body.len() == 1));
}

#[test]
fn test_ir_conversion_discover_agents() {
    let mut converter = setup_converter();

    // DiscoverAgents expression
    let discover_expr = DiscoverAgentsExpr {
        criteria: Box::new(Expression::Literal(Literal::String(
            "capability:weather".to_string(),
        ))),
        options: None,
    };
    let ast = Expression::DiscoverAgents(discover_expr);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert discover agents");
    assert!(matches!(ir, IrNode::DiscoverAgents { .. }));
}

#[test]
fn test_ir_conversion_log_step() {
    let mut converter = setup_converter();

    // LogStep expression
    let log_expr = LogStepExpr {
        level: Some(Keyword::new("info")),
        values: vec![Expression::Literal(Literal::String(
            "test message".to_string(),
        ))],
        location: None,
    };
    let ast = Expression::LogStep(Box::new(log_expr));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert log step");
    assert!(matches!(ir, IrNode::LogStep { values, .. } 
        if values.len() == 1));
}

#[test]
fn test_ir_conversion_symbol_references() {
    let mut converter = setup_converter();

    // Symbol reference (unknown symbols now compile to a runtime-resolved variable ref)
    let ast = Expression::Symbol(Symbol::new("unknown-symbol"));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert unknown symbol to runtime variable ref");
    assert!(matches!(
        ir,
        IrNode::VariableRef { name, binding_id, .. } if name == "unknown-symbol" && binding_id == 0
    ));

    // In strict mode, unknown symbols should error at conversion time
    let mut strict_converter = IrConverter::new().strict();
    let ast = Expression::Symbol(Symbol::new("unknown-symbol"));
    let result = strict_converter.convert_expression(ast);
    assert!(
        result.is_err(),
        "Strict mode should error on unknown symbols"
    );

    // But built-in symbols should work
    let ast = Expression::Symbol(Symbol::new("+"));
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert built-in symbol");
    assert!(matches!(ir, IrNode::VariableRef { name, .. } 
        if name == "+"));
}

#[test]
fn test_ir_conversion_type_coverage() {
    // This test ensures that the IR converter handles all basic expressions
    // without needing complex setups for advanced patterns
    let mut converter = setup_converter();

    // List expression (treated as function application)
    let ast = Expression::List(vec![
        Expression::Symbol(Symbol::new("+")),
        Expression::Literal(Literal::Integer(1)),
        Expression::Literal(Literal::Integer(2)),
    ]);
    let ir = converter
        .convert_expression(ast)
        .expect("Should convert list as application");
    assert!(matches!(ir, IrNode::Apply { .. }));
}
