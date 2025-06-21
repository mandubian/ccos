// Cross-Module IR Integration Tests
// Tests that verify cross-module function calls work through the IR optimization pipeline

use crate::runtime::{Runtime, RuntimeStrategy};
use crate::runtime::module_runtime::ModuleAwareRuntime;
use crate::runtime::ir_runtime::IrRuntime;
use crate::parser::parse_expression;
use crate::ir_converter::IrConverter;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::module_runtime::ModuleRegistry;
    use crate::runtime::values::Value;
    use crate::runtime::environment::IrEnvironment;

    #[test]
    fn test_cross_module_ir_execution() {
        println!("🧪 Starting cross-module IR execution test...");

        // 1. Set up the ModuleRegistry and IrRuntime
        let mut module_registry = ModuleRegistry::new();
        module_registry.add_module_path(PathBuf::from("test_modules"));
        let mut ir_runtime = IrRuntime::new();

        // 2. Load the module. This will compile and execute its top-level forms.
        println!("📦 Loading math.utils module...");
        let load_result = module_registry.load_module("math.utils", &mut ir_runtime);

        match &load_result {
            Ok(module) => {
                println!("✅ Module loaded successfully: {}", module.metadata.name);
                println!("   📝 Exports: {:?}", module.exports.borrow().keys().collect::<Vec<_>>());
            }
            Err(e) => {
                println!("❌ Failed to load module: {:?}", e);
            }
        }
        assert!(load_result.is_ok(), "Failed to load math.utils module: {:?}", load_result.err());

        // 3. Test qualified symbol resolution directly from the registry
        println!("🔍 Testing qualified symbol resolution...");
        let symbol_resolution = module_registry.resolve_qualified_symbol("math.utils/add");
        match &symbol_resolution {
            Ok(value) => {
                println!("✅ Qualified symbol resolved: {:?}", value);
            }
            Err(e) => {
                println!("❌ Qualified symbol resolution failed: {:?}", e);
            }
        }
        assert!(symbol_resolution.is_ok());

        // 4. Create an expression that uses the loaded module
        println!("📝 Parsing expression with qualified symbol...");
        let program_to_run = r#"(math.utils/add 10 5)"#;
        let parse_result = parse_expression(program_to_run);

        match &parse_result {
            Ok(ast) => println!("✅ Parsing successful: {:?}", ast),
            Err(e) => assert!(false, "Parsing qualified symbol failed: {:?}", e),
        }
        let ast = parse_result.unwrap();

        // 5. Convert the expression to IR using the module registry
        println!("🔄 Converting to IR...");
        let mut ir_converter = IrConverter::with_module_registry(&module_registry);
        let ir_result = ir_converter.convert_expression(ast);
        match &ir_result {
            Ok(ir_node) => println!("✅ IR conversion successful: {:?}", ir_node),
            Err(e) => println!("❌ IR conversion failed: {:?}", e),
        }
        assert!(ir_result.is_ok(), "Failed to convert to IR: {:?}", ir_result.err());
        let ir_node = ir_result.unwrap();

        // 6. Execute the IR node. The runtime needs the module registry to resolve the symbol at runtime.
        println!("🚀 Executing through IR runtime...");
        let mut ir_env = IrEnvironment::new();
        let execution_result = ir_runtime.execute_node(&ir_node, &mut ir_env, false, &module_registry);

        match &execution_result {
            Ok(value) => {
                println!("✅ Execution successful, result: {:?}", value);
                assert_eq!(*value, Value::Integer(15));
            }
            Err(e) => {
                assert!(false, "IR execution failed: {:?}", e);
            }
        }
    }
}
