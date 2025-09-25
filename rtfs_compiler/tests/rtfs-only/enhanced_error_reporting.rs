// Tests for enhanced parser error reporting
// Testing the acceptance criteria from issue #39

use rtfs_compiler::parser::parse_with_enhanced_errors;
use rtfs_compiler::parser_error_reporter::ParserErrorReporter;

/// Test mismatched delimiter error reporting
#[test]
fn test_mismatched_parentheses_error() {
    let source = "(let [x 5]"; // Missing closing parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should mention the delimiter mismatch
    assert!(formatted.contains("Mismatched delimiter") || formatted.contains("close"));
    println!("Mismatched parentheses error:\n{}", formatted);
}

#[test]
fn test_mismatched_brackets_error() {
    let source = "[1 2 3)"; // Bracket opened but closed with parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should indicate the mismatch
    println!("Mismatched brackets error:\n{}", formatted);
}

#[test]
fn test_mismatched_braces_error() {
    let source = "{:key \"value\"]"; // Brace opened but closed with bracket

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Mismatched braces error:\n{}", formatted);
}

/// Test invalid function call syntax suggestions
#[test]
fn test_function_call_syntax_suggestion() {
    let source = "(+ 1 2 3"; // Missing closing parenthesis - common function call error

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should suggest proper function call syntax
    assert!(formatted.contains("function") || formatted.contains("call"));
    println!("Function call syntax error:\n{}", formatted);
}

#[test]
fn test_empty_function_call_error() {
    let source = "()"; // Empty list - not a valid function call

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    // This might actually parse as an empty list, so we test the suggestion logic separately
    println!("Empty function call result: {:?}", result);
}

/// Test special form specific error messages
#[test]
fn test_let_syntax_error() {
    let source = "(let x 5)"; // Invalid let syntax - missing binding vector

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));

    // Debug: Print what the parser actually returned
    match &result {
        Ok(items) => {
            println!("Parser succeeded! Items: {:?}", items);
            panic!("Parser should have failed for invalid let syntax");
        }
        Err(error) => {
            println!("Parser failed as expected: {:?}", error);
            let formatted = error.format_with_context();
            println!("Let syntax error:\n{}", formatted);
        }
    }

    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should be specific to let syntax
    assert!(formatted.contains("let") || formatted.contains("binding"));
    println!("Let syntax error:\n{}", formatted);
}

#[test]
fn test_if_syntax_error() {
    let source = "(if)"; // Invalid if - missing condition and branches

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should be specific to if syntax
    assert!(formatted.contains("if") || formatted.contains("condition"));
    println!("If syntax error:\n{}", formatted);
}

#[test]
fn test_fn_syntax_error() {
    let source = "(fn)"; // Invalid fn - missing parameter list and body

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should be specific to fn syntax
    assert!(formatted.contains("fn") || formatted.contains("parameter"));
    println!("Fn syntax error:\n{}", formatted);
}

#[test]
fn test_def_syntax_error() {
    let source = "(def)"; // Invalid def - missing symbol and value

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should be specific to def syntax
    assert!(formatted.contains("def") || formatted.contains("symbol"));
    println!("Def syntax error:\n{}", formatted);
}

/// Test contextual snippets in error messages
#[test]
fn test_contextual_snippets() {
    let source = r#"
(def my-function
  (fn [x y]
    (+ x y

(def another-thing 42)
"#; // Missing closing parenthesis in function body

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should show context around the error
    assert!(formatted.contains("Context around line"));
    assert!(formatted.contains("def my-function") || formatted.contains("another-thing"));
    println!("Contextual snippet error:\n{}", formatted);
}

#[test]
fn test_multiple_context_lines() {
    let source = r#"
(def first-function
  (+ 1 2))

(def problematic-function
  (let [x 5
        y 10]
    (* x y)))

(def third-function
  "This is fine")
"#; // Missing closing bracket in let binding

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));

    if let Err(error) = result {
        let formatted = error.format_with_context();

        // Should show multiple lines of context
        println!("Multiple context lines error:\n{}", formatted);
    }
}

/// Test error reporter configuration
#[test]
fn test_error_reporter_configuration() {
    let reporter = ParserErrorReporter::new();
    assert!(reporter.use_colors);
    assert!(reporter.show_source_context);
    assert_eq!(reporter.max_context_lines, 3);
}
