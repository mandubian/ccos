//! Pure RTFS test utilities that don't depend on CCOS components
//! 
//! These utilities use PureHost and are designed for testing RTFS language
//! features in isolation without CCOS orchestration, capabilities, or external dependencies.

use crate::ir::converter::IrConverter;
use crate::parser;
use crate::runtime::{
    evaluator::Evaluator, ir_runtime::IrRuntime, module_runtime::ModuleRegistry,
    values::Value, pure_host::PureHost, security::RuntimeContext,
};
use crate::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::Arc;

/// Creates a standard module registry for pure RTFS testing.
pub fn create_pure_module_registry() -> ModuleRegistry {
    let registry = ModuleRegistry::new();
    // Note: We are not loading stdlib here by default for pure tests.
    // Tests that need stdlib should load it explicitly.
    registry
}

/// Creates a new AST evaluator with PureHost for pure RTFS testing.
pub fn create_pure_evaluator() -> Evaluator {
    let module_registry = ModuleRegistry::new();
    let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let security_context = RuntimeContext::pure();
    let host = Arc::new(PureHost::new());
    
    Evaluator::new(Arc::new(module_registry), de, security_context, host)
}

/// Creates a new AST evaluator with a provided RuntimeContext for pure RTFS testing.
pub fn create_pure_evaluator_with_context(ctx: RuntimeContext) -> Evaluator {
    let module_registry = ModuleRegistry::new();
    let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let host = Arc::new(PureHost::new());
    
    Evaluator::new(Arc::new(module_registry), de, ctx, host)
}

/// Creates a new AST evaluator with stdlib loaded for pure RTFS testing.
pub fn create_pure_evaluator_with_stdlib() -> Evaluator {
    let mut module_registry = ModuleRegistry::new();
    crate::runtime::stdlib::load_stdlib(&mut module_registry)
        .expect("Failed to load stdlib");
    
    let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let security_context = RuntimeContext::pure();
    let host = Arc::new(PureHost::new());
    
    Evaluator::new(Arc::new(module_registry), de, security_context, host)
}

/// Creates a new IR runtime with PureHost for pure RTFS testing.
pub fn create_pure_ir_runtime() -> IrRuntime {
    let module_registry = Arc::new(ModuleRegistry::new());
    let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let security_context = RuntimeContext::pure();
    let host = Arc::new(PureHost::new());
    
    IrRuntime::new(de, host, security_context)
}

/// Parses and evaluates RTFS code using pure evaluator (no CCOS dependencies).
pub fn parse_and_evaluate_pure(input: &str) -> crate::runtime::error::RuntimeResult<Value> {
    let parsed = parser::parse(input).expect("Failed to parse");
    let evaluator = create_pure_evaluator();
    
    if let Some(last_item) = parsed.last() {
        match last_item {
            crate::ast::TopLevel::Expression(expr) => evaluator.evaluate(expr),
            _ => Ok(Value::String("object_defined".to_string())),
        }
    } else {
        Ok(Value::Nil)
    }
}

/// Parses and evaluates RTFS code with stdlib loaded using pure evaluator.
pub fn parse_and_evaluate_pure_with_stdlib(input: &str) -> crate::runtime::error::RuntimeResult<Value> {
    let parsed = parser::parse(input).expect("Failed to parse");
    let evaluator = create_pure_evaluator_with_stdlib();
    
    if let Some(last_item) = parsed.last() {
        match last_item {
            crate::ast::TopLevel::Expression(expr) => evaluator.evaluate(expr),
            _ => Ok(Value::String("object_defined".to_string())),
        }
    } else {
        Ok(Value::Nil)
    }
}

/// Creates a pure evaluator with controlled security context (no capabilities).
pub fn create_pure_sandboxed_evaluator() -> Evaluator {
    create_pure_evaluator_with_context(RuntimeContext::controlled(vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_pure_evaluator() {
        let evaluator = create_pure_evaluator();
        // Should be able to create evaluator without CCOS dependencies
        assert!(evaluator.env.symbol_names().is_empty());
    }

    #[test]
    fn test_parse_and_evaluate_pure() {
        let result = parse_and_evaluate_pure("42");
        assert_eq!(result.unwrap(), Value::Integer(42));
    }

    #[test]
    fn test_parse_and_evaluate_pure_with_stdlib() {
        let result = parse_and_evaluate_pure_with_stdlib("(+ 1 2)");
        assert_eq!(result.unwrap(), Value::Integer(3));
    }

    #[test]
    fn test_pure_evaluator_capability_error() {
        let evaluator = create_pure_evaluator();
        let parsed = parser::parse("(call :test.capability [])").expect("parse");
        
        if let crate::ast::TopLevel::Expression(expr) = &parsed[0] {
            let result = evaluator.evaluate(expr);
            // Should return an error since capabilities are not available in pure mode
            assert!(result.is_err());
        }
    }
}
