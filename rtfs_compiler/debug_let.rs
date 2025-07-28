use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::Runtime;
use std::rc::Rc;

fn main() {
    let mut module_registry = ModuleRegistry::new();
    let _ = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry);
    let module_registry = Rc::new(module_registry);

    let mut runtime = rtfs_compiler::runtime::Runtime::new_with_tree_walking_strategy(module_registry.clone());

    // Test the specific failing case
    let test_code = "(let [x 5] (let [y (* x 2)] (+ x y)))";
    
    println!("Testing: {}", test_code);
    
    match parse_expression(test_code) {
        Ok(expr) => {
            println!("Parsed successfully: {:?}", expr);
            match runtime.run(&expr) {
                Ok(result) => println!("Result: {}", result),
                Err(e) => println!("Error: {:?}", e),
            }
        }
        Err(e) => println!("Parse error: {:?}", e),
    }
}
