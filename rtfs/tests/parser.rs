#![allow(unused_variables, dead_code)]
// tests/parser.rs

use std::collections::HashMap;
// Import the main parser function
use rtfs::parser::parse;
// Import the AST nodes we need to check against
use rtfs::ast::{Expression, Literal, MapKey, ResourceDefinition, Symbol, TopLevel};
use rtfs::error_reporting::SourceSpan;

// A helper to create a dummy span for tests where we don't care about the exact location.
fn dummy_span() -> SourceSpan {
    SourceSpan::new(0, 0, 0, 0)
}

#[test]
fn test_parse_simple_resource() {
    let input = r#"
    (resource my-simple-resource@1.0.0
        (property :config (map "key" "value"))
    )
    "#;

    let mut properties = HashMap::new();
    let mut map_literal = HashMap::new();
    map_literal.insert(
        MapKey::String("key".to_string()),
        Expression::Literal(Literal::String("value".to_string())),
    );
    properties.insert("config".to_string(), Expression::Map(map_literal));

    let expected_ast = vec![TopLevel::Resource(ResourceDefinition {
        name: Symbol("my-simple-resource".to_string()),
        properties: vec![], // Will be populated by parser
    })];

    let result = parse(input);

    // We need to ignore the spans for comparison as they are not stable across test runs.
    // A more robust way would be to have a function that strips spans from the AST.
    // For now, we'll just compare the important parts.

    match result {
        Ok(ast) => {
            if let Some(TopLevel::Resource(res)) = ast.get(0) {
                assert_eq!(res.name, Symbol("my-simple-resource".to_string()));
                assert_eq!(res.properties.len(), 1);
            } else {
                panic!("Expected a Resource to be parsed");
            }
        }
        Err(e) => {
            panic!("Parsing failed: {:?}", e);
        }
    }
}

// Helper macro for asserting expression parsing
macro_rules! assert_expr_parses_to {
    ($input:expr, $expected:expr) => {
        let ast_result = rtfs::parser::parse_expression($input);
        assert!(
            ast_result.is_ok(),
            "Failed to build expression (parse_expression):
Input: {:?}
Error: {:?}",
            $input,
            ast_result.err().unwrap()
        );
        let ast = ast_result.unwrap();
        if ast != $expected {
            // Use pretty assert for better diffs
            pretty_assertions::assert_eq!(
                ast,
                $expected,
                "Expression AST mismatch for input: {:?}",
                $input
            );
        }
    };
}

#[test]
fn test_parse_simple_literals() {
    assert_expr_parses_to!("123", Expression::Literal(Literal::Integer(123)));
    assert_expr_parses_to!("-45", Expression::Literal(Literal::Integer(-45)));
    assert_expr_parses_to!("1.23", Expression::Literal(Literal::Float(1.23)));
    assert_expr_parses_to!("-0.5", Expression::Literal(Literal::Float(-0.5)));
    assert_expr_parses_to!(
        r#""hello""#,
        Expression::Literal(Literal::String("hello".to_string()))
    );
    assert_expr_parses_to!(
        r#""hello\\world\n""#,
        Expression::Literal(Literal::String("hello\\world\n".to_string()))
    );
    assert_expr_parses_to!("true", Expression::Literal(Literal::Boolean(true)));
    assert_expr_parses_to!("false", Expression::Literal(Literal::Boolean(false)));
    assert_expr_parses_to!("nil", Expression::Literal(Literal::Nil));
}

#[test]
fn test_parse_symbol_keyword() {
    assert_expr_parses_to!(
        "my-symbol",
        Expression::Symbol(rtfs::ast::Symbol("my-symbol".to_string()))
    );
    assert_expr_parses_to!(
        "my-namespace/my-symbol",
        Expression::Symbol(rtfs::ast::Symbol("my-namespace/my-symbol".to_string()))
    );
    assert_expr_parses_to!(
        ":my-keyword",
        Expression::Literal(Literal::Keyword(rtfs::ast::Keyword(
            "my-keyword".to_string()
        )))
    );
}

#[test]
fn test_parse_collections() {
    // Vector
    assert_expr_parses_to!(
        "[1 2 3]",
        Expression::Vector(vec![
            Expression::Literal(Literal::Integer(1)),
            Expression::Literal(Literal::Integer(2)),
            Expression::Literal(Literal::Integer(3)),
        ])
    );
    assert_expr_parses_to!("[]", Expression::Vector(vec![]));

    // List (Function Call heuristic)
    assert_expr_parses_to!(
        "(+ 1 2)",
        Expression::FunctionCall {
            callee: Box::new(Expression::Symbol(rtfs::ast::Symbol("+".to_string()))),
            arguments: vec![
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
            ]
        }
    );
}
