// Manual test for recursive functions
use rtfs_compiler::*;
use rtfs_compiler::runtime::evaluator::Evaluator;

fn main() {
    let tests = vec![
        ("mutual_recursion", include_str!("../../tests/rtfs_files/test_mutual_recursion.rtfs")),
        ("nested_recursion", include_str!("../../tests/rtfs_files/test_nested_recursion.rtfs")),
        ("higher_order_recursion", include_str!("../../tests/rtfs_files/test_higher_order_recursion.rtfs")),
        ("three_way_recursion", include_str!("../../tests/rtfs_files/test_three_way_recursion.rtfs")),
    ];

    println!("Testing recursive function patterns with AST runtime:");
    println!("{}", "=".repeat(60));

    for (name, code) in tests {
        println!("\n🧪 Testing: {}", name);
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
            }        }
        println!("{}", "-".repeat(40));
    }
}
