use rtfs_compiler::*;

fn main() {
    println!("=== Deep Environment Investigation ===");
    
    // Create evaluator and environment with stdlib
    let evaluator = runtime::evaluator::Evaluator::new();
    let mut env = runtime::stdlib::StandardLibrary::create_global_environment();
    
    println!("\n1. Check if 'fact' builtin exists in global environment:");
    match env.lookup(&ast::Symbol("fact".to_string())) {
        Ok(value) => {
            println!("   Found builtin 'fact': {:?}", value);
            if let runtime::values::Value::Function(runtime::values::Function::Builtin { name, arity, .. }) = value {
                println!("   - Name: {}", name);
                println!("   - Arity: {:?}", arity);
            }
        }
        Err(e) => println!("   No builtin 'fact' found: {:?}", e),
    }
    
    println!("\n2. Parse and evaluate the let expression:");
    let input = r#"(let [fact (fn [n acc]
                               (if (= n 0)
                                 acc
                                 (fact (- n 1) (* acc n))))]
                     fact)"#; // Return the function itself, not call it
    
    match parser::parse_expression(input) {
        Ok(expr) => {
            println!("   Parsed successfully");
            
            match evaluator.evaluate_with_env(&expr, &mut env) {
                Ok(result) => {
                    println!("   Evaluation result: {:?}", result);
                    
                    // Now let's check what 'fact' resolves to in the local environment
                    println!("\n3. Check 'fact' lookup after let binding:");
                    match env.lookup(&ast::Symbol("fact".to_string())) {
                        Ok(value) => {
                            println!("   Found 'fact' in environment: {:?}", value);
                            if let runtime::values::Value::Function(func) = &value {
                                match func {
                                    runtime::values::Function::Builtin { name, arity, .. } => {
                                        println!("   - Type: Builtin");
                                        println!("   - Name: {}", name);
                                        println!("   - Arity: {:?}", arity);
                                    }
                                    runtime::values::Function::UserDefined { params, .. } => {
                                        println!("   - Type: UserDefined");
                                        println!("   - Params count: {}", params.len());
                                    }
                                }
                            }
                        }
                        Err(e) => println!("   Error looking up 'fact': {:?}", e),
                    }
                }
                Err(e) => println!("   Evaluation failed: {:?}", e),
            }
        }
        Err(e) => println!("   Parse error: {:?}", e),
    }
    
    println!("\n4. Test simple function call with explicit environment:");
    let simple_call = "(fact 5 1)";
    match parser::parse_expression(simple_call) {
        Ok(expr) => {
            println!("   Parsed function call");
            match evaluator.evaluate_with_env(&expr, &mut env) {
                Ok(result) => println!("   Call result: {:?}", result),
                Err(e) => println!("   Call failed: {:?}", e),
            }
        }
        Err(e) => println!("   Parse error: {:?}", e),
    }
}
