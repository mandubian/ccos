use rtfs_compiler::parser::parse;
use rtfs_compiler::runtime::{Evaluator, Environment};
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::Value;
use std::rc::Rc;

fn main() {
    let code = r#"
(let [fact (fn [n acc]
             (if (= n 0)
               acc
               (fact (- n 1) (* acc n))))]
  fact)
"#;
    
    println!("=== Debug Environment Analysis ===");
    
    // Parse the code
    match parse(code) {
        Ok(ast) => {
            println!("AST parsed successfully");
            
            // Create evaluator
            let evaluator = Evaluator::new();
            
            // Extract the first expression from the AST
            if let Some(TopLevel::Expression(expr)) = ast.first() {
                // Try to evaluate
                match evaluator.evaluate(expr) {
                    Ok(result) => {
                        println!("\nResult: {:?}", result);
                        
                        // If it's a function, let's examine its closure
                        if let Value::Function(func) = &result {
                            match func {
                                rtfs_compiler::runtime::values::Function::UserDefined { 
                                    params, 
                                    variadic_param, 
                                    body, 
                                    closure 
                                } => {
                                    println!("\n=== Function Details ===");
                                    println!("Params: {:?}", params);
                                    println!("Variadic: {:?}", variadic_param);
                                    println!("Body: {:?}", body);
                                    
                                    println!("\n=== Closure Environment ===");
                                    println!("Current bindings: {:?}", closure.current_bindings().keys().collect::<Vec<_>>());
                                    
                                    // Try to look up 'fact' in the closure
                                    let fact_symbol = rtfs_compiler::ast::Symbol("fact".to_string());
                                    match closure.lookup(&fact_symbol) {
                                        Ok(value) => println!("'fact' resolves to: {:?}", value),
                                        Err(err) => println!("'fact' lookup error: {:?}", err),
                                    }
                                }
                                _ => println!("Built-in function"),
                            }
                        }
                    },
                    Err(err) => {
                        println!("\nError: {:?}", err);
                    }
                }
            } else {
                println!("No expression found in AST");
            }
        },
        Err(err) => {
            println!("Parse error: {:?}", err);
        }
    }
}
