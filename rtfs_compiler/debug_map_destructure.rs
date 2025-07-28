use rtfs_compiler::ir::converter::IrConverter;
use rtfs_compiler::parser::{parse_expression};
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::Runtime;
use std::rc::Rc;

fn main() {
    let code = r#"(let [{name :name} {:name "Bob"}] name)"#;
    
    println!("Testing map destructuring: {}", code);
    
    // Parse the code
    let ast = parse_expression(code);
    match ast {
        Ok(ast) => {
            println!("AST: {:#?}", ast);
            
            // Convert to IR
            let mut converter = IrConverter::new();
            match converter.convert_expression(ast.clone()) {
                Ok(ir) => {
                    println!("IR: {:#?}", ir);
                    
                    // Test with tree-walking strategy
                    let module_registry = Rc::new(ModuleRegistry::new());
                    let mut runtime = Runtime::new_with_tree_walking_strategy(module_registry.clone());
                    match runtime.run(&ast) {
                        Ok(result) => println!("Tree-walking result: SUCCESS -> {}", result),
                        Err(e) => println!("Tree-walking result: ERROR -> {}", e),
                    }
                    
                    // Test with IR strategy
                    let ir_strategy = rtfs_compiler::runtime::ir_runtime::IrStrategy::new((*module_registry).clone());
                    let mut ir_runtime = Runtime::new(Box::new(ir_strategy));
                    match ir_runtime.run(&ast) {
                        Ok(result) => println!("IR result: SUCCESS -> {}", result),
                        Err(e) => println!("IR result: ERROR -> {}", e),
                    }
                }
                Err(e) => {
                    println!("IR conversion error: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("Parse error: {:?}", e);
            println!("Error details: {:?}", e);
        }
    }
}
