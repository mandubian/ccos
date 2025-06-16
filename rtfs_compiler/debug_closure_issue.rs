use rtfs_compiler::parser::parse;
use rtfs_compiler::runtime::Evaluator;
use rtfs_compiler::ast::TopLevel;

fn main() {
    let code = r#"
(let [fact (fn [n]
    (if (< n 2)
        1
        (* n (fact (- n 1)))))]
  (fact 5))
"#;
    
    println!("=== Debugging Closure Issue ===");
    
    // Parse the code
    match parse(code) {
        Ok(ast) => {
            println!("AST parsed successfully");
            
            // Create evaluator with stdlib
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
