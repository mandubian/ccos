use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::evaluator::Evaluator;

fn main() {
    // Test a simpler version first to isolate the issue
    let source = r#"(let [fact (fn [n acc]
                             (if (= n 0)
                               acc
                               (fact (- n 1) (* acc n))))]
                  (fact 5 1))"#;

    println!("=== Debug Test Simple Recursive Function ===");
    
    match parse_expression(source) {
        Ok(ast) => {
            println!("AST parsed successfully");
            let evaluator = Evaluator::new();
            match evaluator.evaluate(&ast) {
                Ok(result) => {
                    println!("Result: {:?}", result);
                },
                Err(error) => {
                    println!("Error: {:?}", error);
                }
            }
        },
        Err(error) => {
            println!("Parse error: {:?}", error);
        }
    }
}
