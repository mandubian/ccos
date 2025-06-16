use rtfs_compiler::*;

fn main() {
    println!("=== RTFS Function Parameter Debug ===");
      // Test parsing the exact function definition from the failing test
    let input = r#"(let [fact (fn [n acc]
             (if (= n 0)
               acc
               (fact (- n 1) (* acc n))))]
  (fact 5 1))"#;

    println!("Input: {}", input);
    
    match parser::parse_expression(input) {
        Ok(expr) => {
            println!("Parsed AST: {:#?}", expr);
            
            // Let's extract and examine the function definition
            if let ast::Expression::Let(let_expr) = &expr {
                for binding in &let_expr.bindings {
                    println!("\nBinding pattern: {:?}", binding.pattern);
                    println!("Binding value: {:#?}", binding.value);
                    
                    if let ast::Expression::Fn(fn_expr) = binding.value.as_ref() {
                        println!("\n=== Function Expression Analysis ===");
                        println!("Function params count: {}", fn_expr.params.len());
                        for (i, param) in fn_expr.params.iter().enumerate() {
                            println!("  Param {}: {:?}", i, param);
                        }
                        println!("Variadic param: {:?}", fn_expr.variadic_param);
                        println!("Return type: {:?}", fn_expr.return_type);
                        println!("Body expressions count: {}", fn_expr.body.len());
                    }
                }
            }
        }
        Err(e) => {
            println!("Parse error: {:?}", e);
        }
    }
}
