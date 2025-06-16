use rtfs_compiler::parser::parse;
use rtfs_compiler::runtime::Evaluator;
use rtfs_compiler::ast::TopLevel;

fn main() {
    let code = r#"
(let [x 42]
  x)
"#;
    
    println!("=== Simple Let Binding Test ===");
    
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
    
    println!("\n=== Simple Non-Recursive Function Test ===");
    
    let code2 = r#"
(let [add1 (fn [x] (+ x 1))]
  (add1 5))
"#;
    
    match parse(code2) {
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
