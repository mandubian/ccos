// Cross-Module IR Integration Tests
// Tests that verify cross-module function calls work through the IR optimization pipeline


#[cfg(test)]
mod tests {
    use crate::runtime::stdlib;
    use crate::runtime::values::Value;
    use crate::tests::test_utils::{create_test_module_registry, create_test_ir_runtime, execute_ir_code};

    #[test]
    fn test_cross_module_ir_execution() {
        println!("🧪 Starting cross-module IR execution test...");

        // 1. Set up the environment using test utilities
        let mut module_registry = create_test_module_registry();
        stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib for test");
        let mut ir_runtime = create_test_ir_runtime();

        // 2. Test using stdlib functions directly
        println!("📦 Testing stdlib module access...");

        // 3. Define the code to run that uses the standard library
        let code = "(+ 10 5)";  // Use a simple stdlib function
        println!("🚀 Executing code: {}", code);

        // 4. Execute the code using the helper function, which handles parsing, IR conversion, and execution.
        let result = execute_ir_code(&mut ir_runtime, &mut module_registry, code);

        // 5. Assert the result
        match result {
            Ok(value) => {
                println!("✅ Execution successful, result: {:?}", value);
                assert_eq!(value, Value::Integer(15));
            }
            Err(e) => {
                panic!("❌ Cross-module IR execution failed: {}", e);
            }
        }
    }
}
