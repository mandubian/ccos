use rtfs_compiler::*;

fn main() {    let source = r#"(let [numbers [1 2 3]
        squared (map (fn [x] (* x x)) numbers)]
  {:max-value (reduce max numbers)
   :squared squared})"#;

    match parser::parse_expression(source) {
        Ok(parsed) => {
            println!("Parsed successfully");
            let evaluator = runtime::evaluator::Evaluator::new();
            match evaluator.evaluate(&parsed) {
                Ok(value) => println!("Evaluated successfully: {:?}", value),
                Err(e) => println!("Error during evaluation: {:?}", e),
            }
        }
        Err(e) => println!("Parse error: {:?}", e),
    }
}
