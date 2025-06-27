// rtfs_compiler/src/tests/test_utils.rs
// This file will contain common utilities for setting up test environments.

use std::rc::Rc;
use crate::runtime::{
    evaluator::Evaluator,
    ir_runtime::IrRuntime,
    module_runtime::ModuleRegistry,
    values::Value,
    stdlib,
};
use crate::ir_converter::IrConverter;
use crate::parser;

/// Creates a standard module registry for testing.
pub fn create_test_module_registry() -> ModuleRegistry {
    let registry = ModuleRegistry::new();
    // Note: We are not loading stdlib here by default. 
    // Tests that need stdlib should load it explicitly.
    registry
}

/// Creates a new AST evaluator with the standard library loaded.
pub fn create_test_evaluator() -> Evaluator {
    let mut module_registry = create_test_module_registry();
    stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
    Evaluator::new(Rc::new(module_registry))
}

/// Creates a new IR runtime.
pub fn create_test_ir_runtime() -> IrRuntime {
    IrRuntime::new()
}

/// A helper to parse, convert to IR, and execute code using the IR runtime.
pub fn execute_ir_code(
    runtime: &mut IrRuntime, 
    module_registry: &mut ModuleRegistry, 
    code: &str
) -> Result<Value, String> {
    // Parse the code to get TopLevel AST nodes
    let top_level_forms = match parser::parse(code) {
        Ok(forms) => forms,
        Err(e) => return Err(format!("Parse error: {:?}", e)),
    };

    // Convert the first form to an expression if needed
    let expr = match top_level_forms.first() {
        Some(crate::ast::TopLevel::Expression(expr)) => expr.clone(),
        Some(other) => return Err(format!("Expected expression, got: {:?}", other)),
        None => return Err("No forms found".to_string()),
    };

    // Convert expression to IR
    let mut converter = IrConverter::with_module_registry(module_registry);
    let ir = match converter.convert(&expr) {
        Ok(ir) => ir,
        Err(e) => return Err(format!("IR conversion error: {:?}", e)),
    };

    // Execute the IR node
    runtime.execute_program(&ir, module_registry)
        .map_err(|e| format!("Runtime error: {:?}", e))
}
