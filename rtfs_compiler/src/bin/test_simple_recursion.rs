// Simple test for mutual recursion only
use rtfs_compiler::*;
use rtfs_compiler::runtime::evaluator::Evaluator;

fn main() {    let code = r#"(let [is-even (fn [n]
                (if (= n 0)
                  true
                  (is-odd (- n 1))))
      is-odd (fn [n]
               (if (= n 0)
                 false
                 (is-even (- n 1))))]
  (vector (is-even 4) (is-odd 4) (is-even 7) (is-odd 7)))"#;

    println!("Testing mutual recursion pattern:");
    println!("Code: {}", code.trim());
    
    match parser::parse_expression(code) {
        Ok(parsed) => {
            println!("✅ Parse successful");
            
            let evaluator = Evaluator::new();
            match evaluator.evaluate(&parsed) {
                Ok(result) => {
                    println!("✅ Evaluation successful");
                    println!("Result: {:?}", result);
                }
                Err(e) => {
                    println!("❌ Evaluation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Parse failed: {:?}", e);
        }
    }
}
