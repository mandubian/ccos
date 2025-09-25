// Example demonstrating enhanced parser error reporting

use rtfs_compiler::parser::parse_with_enhanced_errors;

fn main() {
    println!("Testing Enhanced Parser Error Reporting\n");

    // Test cases for different error types
    let test_cases = [
        ("Mismatched Parentheses", "(let [x 5]"),
        ("Mismatched Brackets", "[1 2 3)"),
        ("Invalid Let Syntax", "(let x 5)"),
        ("Invalid If Syntax", "(if)"),
        ("Invalid Fn Syntax", "(fn)"),
        ("Empty Function Call", "()"),
        ("Unclosed Function Call", "(+ 1 2"),
    ];

    for (test_name, source) in &test_cases {
        println!("=== {} ===", test_name);

        match parse_with_enhanced_errors(source, Some("test.rtfs")) {
            Ok(_) => println!("✅ Parsed successfully (unexpected)"),
            Err(error) => {
                println!("{}", error.format_with_context());
            }
        }

        println!();
    }

    // Test multiline error with context
    println!("=== Multiline Context Example ===");
    let multiline_source = r#"
(def my-function
  (fn [x y]
    (+ x y

(def another-thing 42)
"#;

    match parse_with_enhanced_errors(multiline_source, Some("multiline.rtfs")) {
        Ok(_) => println!("✅ Parsed successfully (unexpected)"),
        Err(error) => {
            println!("{}", error.format_with_context());
        }
    }
}
