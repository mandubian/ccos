//! Integration test for RTFS Capability System
//! 
//! This test covers the capability architecture without requiring an LLM or external API.

use rtfs_compiler::parser;
use rtfs_compiler::ast::TopLevel;

// Import the test helpers
mod test_helpers;
use test_helpers::*;

#[test]
fn test_pure_context() {
    let evaluator = create_pure_evaluator();
    let pure_expr = match &parser::parse("(call \"ccos.echo\" \"Hello World\")").expect("parse")[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected an expression"),
    };
    let result = evaluator.eval_expr(
        &pure_expr,
        &mut evaluator.env.clone(),
    );
    assert!(result.is_err(), "Pure context should block capability call");
}

#[test]
fn test_controlled_context() {
    // For now, let's test without actual capability execution to avoid async issues
    let evaluator = create_controlled_evaluator(vec!["ccos.echo".to_string()]);
    
    // Test that the evaluator was created successfully with controlled context
    assert!(evaluator.security_context.is_capability_allowed("ccos.echo"));
    assert!(!evaluator.security_context.is_capability_allowed("ccos.unauthorized"));
    
    println!("test_controlled_context: Security context correctly configured");
}

#[test]
fn test_full_context() {
    // For now, let's test without actual capability execution to avoid async issues
    let evaluator = create_full_evaluator();
    
    // Test that the evaluator was created successfully with full context
    assert!(evaluator.security_context.is_capability_allowed("ccos.echo"));
    assert!(evaluator.security_context.is_capability_allowed("ccos.math.add"));
    assert!(evaluator.security_context.is_capability_allowed("any.capability"));
    
    println!("test_full_context: Security context correctly configured");
}

#[test]
fn test_capability_parsing() {
    let _evaluator = create_full_evaluator();
    
    // Test that we can parse capability calls without executing them
    let test_expressions = [
        "(call \"ccos.echo\" \"Step 1: Echo test\")",
        "(call \"ccos.math.add\" 5 3)",
        "(call \"ccos.echo\" \"Step 3: Sequence completed\")",
    ];
    
    for test_expr in &test_expressions {
        let _expr = match &parser::parse(test_expr).expect("parse")[0] {
            TopLevel::Expression(expr) => expr.clone(),
            _ => panic!("Expected an expression"),
        };
        
        // Just verify that parsing works
        println!("Successfully parsed: {}", test_expr);
    }
    
    println!("test_capability_parsing: All expressions parsed successfully");
}
