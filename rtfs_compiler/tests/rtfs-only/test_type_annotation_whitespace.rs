use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::*;
use std::sync::Arc;

fn test_parse_and_execute(code: &str, test_name: &str) -> (bool, String) {
    // Parse the code
    let parsed = match parser::parse_expression(code) {
        Ok(ast) => ast,
        Err(e) => return (false, format!("Parse error: {:?}", e)),
    };

    println!("   Parsed {} successfully", test_name); // Test AST runtime
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let capability_marketplace = std::sync::Arc::new(
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry),
    );
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap(),
    ));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    let ast_result = match evaluator.evaluate(&parsed) {
        Ok(value) => {
            println!("   ✓ AST runtime executed: {:?}", value);
            true
        }
        Err(e) => {
            println!("   ✗ AST runtime failed: {}", e);
            false
        }
    }; // Test IR runtime
    let mut converter = rtfs_compiler::ir::converter::IrConverter::new();
    let ir_result = match converter.convert_expression(parsed.clone()) {
        Ok(_ir_node) => {
            let module_registry = ModuleRegistry::new();
            let mut ir_strategy = runtime::ir_runtime::IrStrategy::new(module_registry);
            match ir_strategy.run(&parsed) {
                Ok(value) => {
                    println!("   ✓ IR runtime executed: {:?}", value);
                    true
                }
                Err(e) => {
                    println!("   ✗ IR runtime failed: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            println!("   ✗ IR conversion failed: {:?}", e);
            false
        }
    };

    let success = ast_result && ir_result;
    let message = if success {
        "Success".to_string()
    } else {
        "Failed".to_string()
    };
    (success, message)
}

#[test]
fn test_type_annotation_whitespace() {
    // Test with whitespace between : and type name
    let code_with_whitespace = "(let [x : Int 42] x)";
    // Test without whitespace between : and type name
    let code_without_whitespace = "(let [x :Int 42] x)";
    // Test with multiple spaces
    let code_with_multiple_spaces = "(let [x    :    Int 42] x)";

    println!("Testing type annotation whitespace handling...");

    // Test parsing with whitespace
    println!("1. Testing with whitespace: {}", code_with_whitespace);
    let (success1, _) = test_parse_and_execute(code_with_whitespace, "with whitespace");

    // Test parsing without whitespace
    println!("2. Testing without whitespace: {}", code_without_whitespace);
    let (success2, _) = test_parse_and_execute(code_without_whitespace, "without whitespace");

    // Test parsing with multiple spaces
    println!(
        "3. Testing with multiple spaces: {}",
        code_with_multiple_spaces
    );
    let (success3, _) = test_parse_and_execute(code_with_multiple_spaces, "with multiple spaces");

    println!("Type annotation whitespace test completed.");

    // Summary
    println!("\nSUMMARY:");
    println!(
        "With whitespace (x : Int): {}",
        if success1 { "✓ PASS" } else { "✗ FAIL" }
    );
    println!(
        "Without whitespace (x :Int): {}",
        if success2 { "✓ PASS" } else { "✗ FAIL" }
    );
    println!(
        "With multiple spaces (x    :    Int): {}",
        if success3 { "✓ PASS" } else { "✗ FAIL" }
    );

    // All should pass since whitespace should be allowed
    assert!(
        success1,
        "Type annotation with whitespace should be allowed"
    );
    assert!(
        success2,
        "Type annotation without whitespace should be allowed"
    );
    assert!(
        success3,
        "Type annotation with multiple spaces should be allowed"
    );
}
