use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::{Symbol, Expression};
use rtfs_compiler::runtime::secure_stdlib;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::agent::discovery_traits::NoOpAgentDiscovery;
use rtfs_compiler::runtime::RuntimeHost;
use rtfs_compiler::ccos::delegation::MockDelegationEngine;
use rtfs_compiler::runtime::execution_context::ExecutionContext;
use std::sync::Arc;
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    env_logger::init();
    
    // Create module registry with standard library
    let mut module_registry = ModuleRegistry::new();
    if let Err(e) = secure_stdlib::register_functions(&mut module_registry) {
        eprintln!("Failed to register stdlib: {}", e);
        return;
    }

    // Create a base environment with stdlib
    let mut env = Environment::new();
    if let Some(stdlib_module) = module_registry.get_module("stdlib") {
        for (name, export) in stdlib_module.exports.borrow().iter() {
            env.define(&Symbol(name.clone()), export.value.clone());
        }
    }

    // Test the nested let expression that's failing
    let test_code = "(let [x 5] (let [y (* x 2)] (+ x y)))";
    
    match parser::parse_expression(test_code) {
        Ok(ast) => {
            println!("Parsed AST: {:?}", ast);
            
            let marketplace = Arc::new(CapabilityMarketplace::new());
            let agent_discovery = Arc::new(NoOpAgentDiscovery);
            let host = Rc::new(RuntimeHost::new(marketplace.clone(), agent_discovery));
            let delegation_engine = Arc::new(MockDelegationEngine::new());
            let context = ExecutionContext::minimal();
            
            let evaluator = Evaluator::new(module_registry, delegation_engine, context, host);
            
            match evaluator.eval(&ast, &mut env) {
                Ok(result) => println!("Result: {}", result),
                Err(e) => println!("Error: {:?}", e),
            }
        }
        Err(e) => println!("Parse error: {:?}", e),
    }
}
