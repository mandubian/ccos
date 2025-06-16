use rtfs_compiler::parser::parse;
use rtfs_compiler::runtime::Evaluator;
use rtfs_compiler::ast::TopLevel;

fn main() {
    let code = r#"
(let [fact (fn [n acc]
             (if (= n 0)
               acc
               (fact (- n 1) (* acc n))))]
  (fact 5 1))
"#;
    
    println!("=== Debugging Recursive Function with 2 Parameters ===");
    
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
}
