//! Integration test for RTFS Capability System
//! 
//! This test covers the capability architecture without requiring an LLM or external API.

use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityPolicies};
use rtfs_compiler::ast::TopLevel;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn test_pure_context() {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let pure_context = RuntimeContext::pure();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        pure_context,
    );
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
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let controlled_context = SecurityPolicies::test_capabilities();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        controlled_context,
    );
    let controlled_expr = match &parser::parse("(call \"ccos.echo\" \"Hello World\")").expect("parse")[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => panic!("Expected an expression"),
    };
    let result = evaluator.eval_expr(
        &controlled_expr,
        &mut evaluator.env.clone(),
    );
    println!("test_controlled_context result: {:?}", result);
    assert!(result.is_ok(), "Controlled context should allow capability call");
}

#[test]
fn test_full_context() {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let full_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        full_context,
    );
    let capabilities_to_test = [
        ("ccos.echo", "\"test input\""),
        ("ccos.math.add", "10 20"),
    ];
    for (capability, input) in &capabilities_to_test {
        let test_expr = format!("(call \"{}\" {})", capability, input);
        let expr = match &parser::parse(&test_expr).expect("parse")[0] {
            TopLevel::Expression(expr) => expr.clone(),
            _ => panic!("Expected an expression"),
        };
        let result = evaluator.eval_expr(
            &expr,
            &mut evaluator.env.clone(),
        );
        println!("test_full_context: {} result: {:?}", capability, result);
        assert!(result.is_ok(), "Full context should allow {}", capability);
    }
}

#[test]
fn test_plan_execution() {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let full_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        full_context,
    );
    let plan_rtfs = r#"
    (plan "test-capability-plan"
      :description "Test plan that uses various capabilities"
      :intent-id "test-intent"
      :steps [
        (call "ccos.echo" "Step 1: Echo test")
        (let [result (call "ccos.math.add" 5 3)]
          (call "ccos.echo" (str "Step 2: Math result is " result)))
        (call "ccos.echo" "Step 3: Plan completed")
      ])
    "#;
    let plan_ast = parser::parse(plan_rtfs).expect("parse");
    let plan_result = evaluator.eval_toplevel(&plan_ast);
    assert!(plan_result.is_ok(), "Plan evaluation should succeed, but failed with: {:?}", plan_result.err());
}
