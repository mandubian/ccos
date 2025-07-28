#[cfg(test)]
mod test_nested_let {
    use rtfs_compiler::parser;
    use rtfs_compiler::runtime::environment::Environment;
    use rtfs_compiler::runtime::evaluator::Evaluator;
    use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
    use rtfs_compiler::runtime::secure_stdlib;
    use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
    use rtfs_compiler::agent::discovery_traits::NoOpAgentDiscovery;
    use rtfs_compiler::runtime::RuntimeHost;
    use rtfs_compiler::ccos::delegation::MockDelegationEngine;
    use rtfs_compiler::runtime::execution_context::ExecutionContext;
    use rtfs_compiler::ast::Symbol;
    use std::sync::Arc;
    use std::rc::Rc;

    #[test]
    fn test_nested_let_basic() {
        let mut module_registry = ModuleRegistry::new();
        secure_stdlib::register_functions(&mut module_registry).unwrap();

        let mut env = Environment::new();
        if let Some(stdlib_module) = module_registry.get_module("stdlib") {
            for (name, export) in stdlib_module.exports.borrow().iter() {
                env.define(&Symbol(name.clone()), export.value.clone());
            }
        }

        let marketplace = Arc::new(CapabilityMarketplace::new());
        let agent_discovery = Arc::new(NoOpAgentDiscovery);
        let host = Rc::new(RuntimeHost::new(marketplace.clone(), agent_discovery));
        let delegation_engine = Arc::new(MockDelegationEngine::new());
        let context = ExecutionContext::minimal();
        let evaluator = Evaluator::new(module_registry, delegation_engine, context, host);

        // Test the exact failing case
        let test_code = "(let [x 5] (let [y (* x 2)] (+ x y)))";
        let ast = parser::parse_expression(test_code).unwrap();
        let result = evaluator.eval(&ast, &mut env);
        
        match result {
            Ok(value) => println!("SUCCESS: {}", value),
            Err(e) => {
                println!("ERROR: {:?}", e);
                panic!("Should have succeeded");
            }
        }
    }
}
