// Simple test program to demonstrate enhanced parser error reporting

use rtfs_compiler::parser::parse_with_enhanced_errors;

fn main() {
    println!("=== Enhanced Parser Error Reporting Demo ===\n");

    // Test 1: Mismatched delimiters
    println!("1. Testing mismatched parentheses:");
    let source1 = "(let [x 5]"; // Missing closing )
    if let Err(error) = parse_with_enhanced_errors(source1, Some("test1.rtfs")) {
        println!("{}\n", error.format_with_context());
    }

    // Test 2: Invalid let syntax
    println!("2. Testing invalid let syntax:");
    let source2 = "(let x 5)"; // Missing binding vector
    if let Err(error) = parse_with_enhanced_errors(source2, Some("test2.rtfs")) {
        println!("{}\n", error.format_with_context());
    }

    // Test 3: Invalid function call
    println!("3. Testing empty function call:");
    let source3 = "()"; // Empty function call
    if let Err(error) = parse_with_enhanced_errors(source3, Some("test3.rtfs")) {
        println!("{}\n", error.format_with_context());
    }

    // Test 4: Multiline context
    println!("4. Testing multiline context:");
    let source4 = r#"
(def my-function
  (fn [x y]
    (+ x y

(def another 42)
"#;
    if let Err(error) = parse_with_enhanced_errors(source4, Some("test4.rtfs")) {
        println!("{}\n", error.format_with_context());
    }
}
