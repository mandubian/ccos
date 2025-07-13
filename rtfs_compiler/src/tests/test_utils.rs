// rtfs_compiler/src/tests/test_utils.rs
// This file will contain common utilities for setting up test environments.

use crate::ir::converter::IrConverter;
use crate::parser;
use crate::runtime::{
    evaluator::Evaluator, ir_runtime::IrRuntime, module_runtime::ModuleRegistry, stdlib,
    values::Value,
};
use std::rc::Rc;
use crate::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::Arc;

/// Creates a standard module registry for testing.
pub fn create_test_module_registry() -> ModuleRegistry {
    let registry = ModuleRegistry::new();
    // Note: We are not loading stdlib here by default.
    // Tests that need stdlib should load it explicitly.
    registry
}

/// Creates a new AST evaluator with the standard library loaded.
pub fn create_test_evaluator() -> Evaluator {
    let module_registry = ModuleRegistry::new();
    let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    Evaluator::new(Rc::new(module_registry), de, crate::runtime::security::RuntimeContext::pure())
}

/// Creates a new IR runtime.
pub fn create_test_ir_runtime() -> crate::runtime::ir_runtime::IrRuntime {
    let delegation_engine = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    crate::runtime::ir_runtime::IrRuntime::new(delegation_engine)
}

/// A helper to parse, convert to IR, and execute code using the IR runtime.
pub fn execute_ir_code(
    runtime: &mut IrRuntime,
    module_registry: &mut ModuleRegistry,
    code: &str,
) -> Result<Value, String> {
    // Parse the code to get TopLevel AST nodes
    let top_level_forms = match parser::parse(code) {
        Ok(forms) => forms,
        Err(e) => return Err(format!("Parse error: {:?}", e)),
    };

    // Convert each top-level form to IR
    let mut converter = IrConverter::with_module_registry(module_registry);
    let mut ir_forms = Vec::new();

    for form in top_level_forms {
        match form {
            crate::ast::TopLevel::Expression(expr) => {
                let ir_node = match converter.convert(&expr) {
                    Ok(ir) => ir,
                    Err(e) => return Err(format!("IR conversion error: {:?}", e)),
                };
                ir_forms.push(ir_node);
            }
            _ => return Err("Only expressions are supported in this test utility".to_string()),
        }
    }

    // Create a program node
    let program_node = crate::ir::core::IrNode::Program {
        id: converter.next_id(),
        version: "1.0".to_string(),
        forms: ir_forms,
        source_location: None,
    };

    // Execute the IR program
    runtime
        .execute_program(&program_node, module_registry)
        .map_err(|e| format!("Runtime error: {:?}", e))
}
