//! Standalone test for RTFS Capability System
//! 
//! This script tests the capability architecture without requiring an LLM or external API.

use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityPolicies};
use rtfs_compiler::ast::TopLevel;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 RTFS Capability System Test");
    println!("===============================\n");

    // Test 1: Pure security context (no capabilities allowed)
    println!("1️⃣ Testing Pure Security Context");
    test_pure_context()?;

    // Test 2: Controlled security context
    println!("\n2️⃣ Testing Controlled Security Context");
    test_controlled_context()?;

    // Test 3: Full security context
    println!("\n3️⃣ Testing Full Security Context");
    test_full_context()?;

    // Test 4: Plan execution with capabilities
    println!("\n4️⃣ Testing Plan Execution with Capabilities");
    test_plan_execution()?;

    println!("\n✅ All capability tests completed!");
    Ok(())
}

fn test_pure_context() -> Result<(), Box<dyn std::error::Error>> {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let pure_context = RuntimeContext::pure();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        pure_context,
    );
    
    // Try to call a capability - should fail
    let pure_expr = match &parser::parse("(call :ccos.echo \"Hello World\")")?[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => return Err("Expected an expression".into()),
    };
    let result = evaluator.eval_expr(
        &pure_expr,
        &mut evaluator.env.clone(),
    );
    
    match result {
        Ok(_) => println!("❌ Pure context incorrectly allowed capability call"),
        Err(e) => println!("✅ Pure context correctly blocked capability: {}", e),
    }
    
    Ok(())
}

fn test_controlled_context() -> Result<(), Box<dyn std::error::Error>> {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let controlled_context = SecurityPolicies::test_capabilities();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        controlled_context,
    );
    
    // Try to call allowed capability
    let controlled_expr = match &parser::parse("(call :ccos.echo \"Hello World\")")?[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => return Err("Expected an expression".into()),
    };
    let result = evaluator.eval_expr(
        &controlled_expr,
        &mut evaluator.env.clone(),
    );
    
    match result {
        Ok(result) => println!("✅ Controlled context allowed capability call: {:?}", result),
        Err(e) => println!("❌ Controlled context incorrectly blocked capability: {}", e),
    }
    
    Ok(())
}

fn test_full_context() -> Result<(), Box<dyn std::error::Error>> {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let full_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        full_context,
    );
    
    // Test various capabilities
    let capabilities_to_test = [
        ("ccos.echo", "\"test input\""),
        ("ccos.math.add", "{:a 10 :b 20}"),
        ("ccos.ask-human", "\"What is your name?\""),
    ];
    
    for (capability, input) in &capabilities_to_test {
        let test_expr = format!("(call :{} {})", capability, input);
        let expr = match &parser::parse(&test_expr)?[0] {
            TopLevel::Expression(expr) => expr.clone(),
            _ => return Err("Expected an expression".into()),
        };
        let result = evaluator.eval_expr(
            &expr,
            &mut evaluator.env.clone(),
        );
        
        match result {
            Ok(value) => println!("✅ Full context allowed {}: {:?}", capability, value),
            Err(e) => println!("❌ Full context failed for {}: {}", capability, e),
        }
    }
    
    Ok(())
}

fn test_plan_execution() -> Result<(), Box<dyn std::error::Error>> {
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let full_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()), 
        stdlib_env,
        delegation,
        full_context,
    );
    
    // Test plan with capability calls
    let plan_rtfs = r#"
    (plan test-capability-plan
      :description "Test plan that uses various capabilities"
      :intent-id "test-intent"
      :steps [
        (call :ccos.echo "Step 1: Echo test")
        (let [result (call :ccos.math.add {:a 5 :b 3})]
          (call :ccos.echo (str "Step 2: Math result is " result)))
        (call :ccos.echo "Step 3: Plan completed")
      ])
    "#;
    
    let plan_ast = parser::parse(plan_rtfs)?;
    
    // Evaluate the plan
    let plan_result = evaluator.eval_toplevel(&plan_ast);
    match plan_result {
        Ok(metadata) => println!("✅ Plan evaluated successfully. Metadata: {:?}", metadata),
        Err(e) => println!("❌ Plan evaluation failed: {}", e),
    }
    
    Ok(())
} 