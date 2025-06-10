mod ast; // Declare the ast module
pub mod parser; // Declare the parser module (now a directory)
pub mod runtime; // Declare the runtime module

use parser::parse_expression;
use runtime::Evaluator;

fn main() {
    println!("RTFS Compiler with Enhanced Runtime");
    println!("===================================");
    
    let evaluator = Evaluator::new();
    
    // Test various expressions including new features
    let test_cases = vec![
        // Basic functionality tests
        "(+ 1 2 3)",
        "(vector 1 2 3)",
        "(conj [1 2] 3 4)",
        "(map :a 1 :b 2)", // Map literal constructor
        
        // Type predicates
        "(nil? nil)",
        "(int? 42)",
        "(string? \"hello\")",
        
        // Tool functions
        "(tool:log \"Hello from RTFS!\")",
        "(tool:current-time)",
        
        // Enhanced tool functions
        "(tool:get-env \"PATH\" \"default\")",
        "(tool:http-fetch \"http://example.com\")",
        "(tool:http-fetch \"http://error.com\")",
        
        // Resource management simulation
        "(tool:open-file \"test.txt\")",
        
        // Let expressions
        "(let [x 10 y 20] (+ x y))",
        
        // If expressions
        "(if true \"yes\" \"no\")",
        "(if false \"no\" \"yes\")",
        
        // Do expressions
        "(do (tool:log \"step 1\") (tool:log \"step 2\") 42)",
        
        // Parallel execution (sequential simulation)
        "(parallel [a (+ 1 2)] [b (* 3 4)])",
        
        // JSON operations
        "(tool:parse-json \"42\")",
        "(tool:serialize-json [1 2 3])",
        
        // Error handling showcase
        "(/ 10 0)", // Division by zero
        
        // Match expressions (basic)
        "(match 42 42 \"found\" _ \"not found\")",
    ];
    
    for (i, input) in test_cases.iter().enumerate() {
        println!("\nTest {}: {}", i + 1, input);
        match parse_expression(input) {
            Ok(ast) => {
                match evaluator.evaluate(&ast) {
                    Ok(result) => println!("  → {}", result.to_string()),
                    Err(error) => println!("  ✗ Runtime Error: {}", error),
                }
            },
            Err(error) => println!("  ✗ Parse Error: {:?}", error),
        }
    }
    
    // Demonstrate with-resource simulation
    println!("\n\nAdvanced Features Demonstration:");
    println!("===============================");
    
    // Test with-resource (would need proper AST parsing for complex expressions)
    println!("\nNote: For complex expressions like with-resource, try-catch, etc.,");
    println!("      full program parsing would be needed rather than single expressions.");
    println!("      The runtime system supports these features when properly parsed.");
    
    // Test advanced runtime features
    test_advanced_features(&evaluator);
}

fn test_advanced_features(evaluator: &Evaluator) {
    println!("\n\nAdvanced Runtime Features Test:");
    println!("==============================");
    
    // Test try-catch simulation (would need full program parsing)
    println!("\nNote: The following would work with full RTFS program parsing:");
    
    println!("  with-resource: (with-resource [f FileHandle (tool:open-file \"data.txt\")] (tool:read-line f))");
    println!("  try-catch: (try (/ 10 0) (catch :error/division-by-zero e (tool:log \"Caught error\")))");
    println!("  match: (match value [:ok data] data [:error err] nil)");
    println!("  fn: (fn [x] (* x x))");
    println!("  defn: (defn square [x] (* x x))");
    
    // Demonstrate error handling types
    println!("\nError Handling Examples:");
    
    // Division by zero
    if let Ok(ast) = parse_expression("(/ 10 0)") {
        match evaluator.evaluate(&ast) {
            Ok(result) => println!("  Division: {}", result.to_string()),
            Err(error) => println!("  Division Error: {}", error),
        }
    }
    
    // Type error
    if let Ok(ast) = parse_expression("(+ \"hello\" 42)") {
        match evaluator.evaluate(&ast) {
            Ok(result) => println!("  Type mix: {}", result.to_string()),
            Err(error) => println!("  Type Error: {}", error),
        }
    }
    
    // Resource operations
    println!("\nResource Management Simulation:");
    if let Ok(ast) = parse_expression("(tool:open-file \"example.txt\" :read)") {
        match evaluator.evaluate(&ast) {
            Ok(result) => {
                println!("  File opened: {}", result.to_string());
                
                // Test reading from the resource (would need to extract handle)
                println!("  Note: In real usage, with-resource would automatically manage the lifecycle");
            },
            Err(error) => println!("  File Error: {}", error),
        }
    }
}
