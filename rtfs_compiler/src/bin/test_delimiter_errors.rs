// Test specific delimiter mismatch cases

use rtfs_compiler::parser::parse_with_enhanced_errors;

fn main() {
    println!("=== Delimiter Mismatch Detection Demo ===\n");
    
    // Test bracket/parenthesis mismatch
    println!("1. Testing bracket/parenthesis mismatch:");
    let source1 = "[1 2 3)"; // [ opened but ) closed
    if let Err(error) = parse_with_enhanced_errors(source1, Some("mismatch1.rtfs")) {
        println!("{}\n", error.format_with_context());
    } else {
        println!("No error detected (parser may handle this gracefully)\n");
    }
    
    // Test nested delimiter mismatch
    println!("2. Testing nested delimiter mismatch:");
    let source2 = "(let [x (+ 1 2] y)"; // ( opened but ] closed
    if let Err(error) = parse_with_enhanced_errors(source2, Some("mismatch2.rtfs")) {
        println!("{}\n", error.format_with_context());
    } else {
        println!("No error detected (parser may handle this gracefully)\n");
    }
    
    // Test unclosed delimiter in multiline
    println!("3. Testing unclosed delimiter in multiline:");
    let source3 = r#"
(def my-function
  (fn [x y
    (+ x y))
  value)
"#; // [ opened but never closed
    if let Err(error) = parse_with_enhanced_errors(source3, Some("unclosed.rtfs")) {
        println!("{}\n", error.format_with_context());
    } else {
        println!("No error detected (parser may handle this gracefully)\n");
    }
}