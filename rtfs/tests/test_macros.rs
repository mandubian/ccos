
use rtfs::parser::parse;
use rtfs::ast::{Expression, Literal, Symbol, TopLevel, DoExpr};
use rtfs::compiler::expander::MacroExpander;

#[test]
fn test_simple_macro() {
    let input = "(do
        (defmacro my-add [a b] `(+ ~a ~b))
        (my-add 1 2)
    )";
    let expected = Expression::Do(DoExpr {
        expressions: vec![
            Expression::Literal(Literal::Nil),
            Expression::FunctionCall {
                callee: Box::new(Expression::Symbol(Symbol::new("+"))),
                arguments: vec![
                    Expression::Literal(Literal::Integer(1)),
                    Expression::Literal(Literal::Integer(2)),
                ],
            },
        ],
    });
    let result = parse(input).unwrap();
    let parsed_expr = match &result[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected expression result")
    };
    
    // Expand macros
    let mut expander = MacroExpander::default();
    let expanded_expr = expander.expand_top_level(&parsed_expr).unwrap();
    
    assert_eq!(expanded_expr, expected);
}

#[test]
fn test_quasiquote() {
    let input = "(do
        (defmacro my-list [a] `(1 ~a 3))
        (my-list 2)
    )";
    let expected = Expression::Do(DoExpr {
        expressions: vec![
            Expression::Literal(Literal::Nil),
            Expression::List(vec![
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
                Expression::Literal(Literal::Integer(3)),
            ]),
        ],
    });
    let result = parse(input).unwrap();
    let parsed_expr = match &result[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected expression result")
    };
    
    // Expand macros
    let mut expander = MacroExpander::default();
    let expanded_expr = expander.expand_top_level(&parsed_expr).unwrap();
    
    assert_eq!(expanded_expr, expected);
}

#[test]
fn test_unquote_splicing() {
    let input = "(do
        (defmacro my-list [a] `(1 ~@a 4))
        (my-list (list 2 3))
    )";
    let expected = Expression::Do(DoExpr {
        expressions: vec![
            Expression::Literal(Literal::Nil),
            Expression::List(vec![
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(1)),
                Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol::new("list"))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(2)),
                        Expression::Literal(Literal::Integer(3)),
                    ],
                },
                Expression::Literal(Literal::Integer(4)),
            ]),
        ],
    });
    let result = parse(input).unwrap();
    let parsed_expr = match &result[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected expression result")
    };
    
    // Expand macros
    let mut expander = MacroExpander::default();
    let expanded_expr = expander.expand_top_level(&parsed_expr).unwrap();
    
    assert_eq!(expanded_expr, expected);
}

#[test]
fn test_recursive_macro() {
    let input = "(do
        (defmacro my-if [cond then else] `(if ~cond ~then ~else))
        (my-if true 1 2)
    )";
    let expected = Expression::Do(DoExpr {
        expressions: vec![
            Expression::Literal(Literal::Nil),
            Expression::If(rtfs::ast::IfExpr {
                condition: Box::new(Expression::Literal(Literal::Boolean(true))),
                then_branch: Box::new(Expression::Literal(Literal::Integer(1))),
                else_branch: Some(Box::new(Expression::Literal(Literal::Integer(2)))),
            }),
        ],
    });
    let result = parse(input).unwrap();
    let parsed_expr = match &result[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected expression result")
    };
    
    // Expand macros
    let mut expander = MacroExpander::default();
    let expanded_expr = expander.expand_top_level(&parsed_expr).unwrap();
    
    assert_eq!(expanded_expr, expected);
}

#[test]
fn test_variadic_macro() {
    let input = "(do
        (defmacro test-variadic [x & rest] `(+ ~x 1))
        (test-variadic 5)
    )";

    // Expected expanded AST: Nil for defmacro, then a function call (+ 5 1)
    let expected = rtfs::ast::Expression::Do(rtfs::ast::DoExpr {
        expressions: vec![
            rtfs::ast::Expression::Literal(rtfs::ast::Literal::Nil),
            rtfs::ast::Expression::FunctionCall {
                callee: Box::new(rtfs::ast::Expression::Symbol(rtfs::ast::Symbol::new("+"))),
                arguments: vec![
                    rtfs::ast::Expression::Literal(rtfs::ast::Literal::Integer(5)),
                    rtfs::ast::Expression::Literal(rtfs::ast::Literal::Integer(1)),
                ],
            },
        ],
    });

    let result = parse(input).unwrap();
    let parsed_expr = match &result[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected expression result"),
    };

    let mut expander = MacroExpander::default();
    let expanded_expr = expander.expand_top_level(&parsed_expr).unwrap();

    assert_eq!(expanded_expr, expected);
}


// Integration test: expand top-levels to capture the MacroExpander, then
// inject that expander into an evaluator and execute a new expression that
// uses the previously defined macro. This verifies the shared-expander path
// used by the compiler driver and runtime strategies.
#[test]
fn test_macro_runtime_integration() {
    let input = "(do
        (defmacro incr [x] `(+ ~x 1))
        (incr 41)
    )";

    // Parse and expand top-levels, capturing the MacroExpander
    let parsed = parse(input).unwrap();
    let (expanded_items, expander) = rtfs::compiler::expander::expand_top_levels(&parsed).unwrap();

    // Evaluate the expanded items using an evaluator (this runs the first program)
    let module_registry = std::sync::Arc::new(rtfs::runtime::module_runtime::ModuleRegistry::new());
    let host = rtfs::runtime::pure_host::create_pure_host();
    let mut evaluator = rtfs::runtime::evaluator::Evaluator::new(
        module_registry.clone(),
        rtfs::runtime::security::RuntimeContext::full(),
        host.clone(),
        rtfs::compiler::expander::MacroExpander::default(),
    );

    // Evaluate the expanded top-levels (should execute and produce 42)
    let outcome = evaluator.eval_toplevel(&expanded_items).unwrap();
    match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(val) => {
            assert_eq!(val, rtfs::runtime::values::Value::Integer(42));
        }
        other => panic!("Expected complete outcome, got: {:?}", other),
    }

    // Now inject the captured MacroExpander into the evaluator and execute a new
    // top-level expression that uses the same macro name (not pre-expanded).
    evaluator.set_macro_expander(expander);

    let new_expr = rtfs::ast::TopLevel::Expression(rtfs::ast::Expression::FunctionCall {
        callee: Box::new(rtfs::ast::Expression::Symbol(rtfs::ast::Symbol::new("incr"))),
        arguments: vec![rtfs::ast::Expression::Literal(rtfs::ast::Literal::Integer(100))],
    });

    let outcome2 = evaluator.eval_toplevel(&[new_expr]).unwrap();
    match outcome2 {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(val) => {
            assert_eq!(val, rtfs::runtime::values::Value::Integer(101));
        }
        other => panic!("Expected complete outcome, got: {:?}", other),
    }
}
