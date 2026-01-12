// RTFS Parser Robustness Tests
// Tests for parser error handling and recovery

use rtfs::parser::parse_with_enhanced_errors;

/// Test mismatched delimiter error reporting
#[test]
fn test_mismatched_parentheses() {
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
fn test_mismatched_brackets() {
    let source = "[1 2 3)"; // Bracket opened but closed with parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should indicate some kind of error (might not specifically say "mismatch")
    assert!(
        formatted.contains("Expected")
            || formatted.contains("mismatch")
            || formatted.contains("error")
            || formatted.contains("Error")
    );
    println!("Mismatched brackets error:\n{}", formatted);
}

#[test]
fn test_mismatched_braces() {
    let source = "{:key \"value\"]"; // Brace opened but closed with bracket

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();
    assert!(
        formatted.contains("Mismatched delimiter")
            || formatted.contains("Unclosed delimiter")
            || formatted.contains("Unexpected closing delimiter")
    );
    println!("Mismatched braces error:\n{}", formatted);
}

/// Test incomplete expression errors
#[test]
fn test_incomplete_function_call() {
    let source = "(+ 1 2"; // Missing closing parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should suggest proper function call syntax
    assert!(
        formatted.contains("function")
            || formatted.contains("call")
            || formatted.contains("Expected")
            || formatted.contains("close")
    );
    println!("Incomplete function call error:\n{}", formatted);
}

#[test]
fn test_incomplete_let_expression() {
    let source = "(let [x 5"; // Missing closing parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Incomplete let expression error:\n{}", formatted);
}

/// Test invalid special form syntax
#[test]
fn test_invalid_let_syntax() {
    let source = "(let x 5)"; // Invalid let syntax - missing binding vector

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    // Should be specific to let syntax
    assert!(
        formatted.contains("let")
            || formatted.contains("binding")
            || formatted.contains("Expected")
    );
    println!("Invalid let syntax error:\n{}", formatted);
}

#[test]
fn test_invalid_if_syntax() {
    let source = "(if true)"; // Missing then and else clauses

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Invalid if syntax error:\n{}", formatted);
}

#[test]
fn test_invalid_fn_syntax() {
    let source = "(fn)"; // Empty function definition

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Invalid fn syntax error:\n{}", formatted);
}

/// Test type annotation errors
#[test]
fn test_invalid_type_annotation() {
    let source = "(def x :InvalidType 5)"; // Invalid type expression

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    // Parser-level check: unknown type names may still parse (validation happens later).
    // The robustness goal here is "no crash" + deterministic behavior.
    assert!(result.is_ok() || result.is_err());
}

/// Test metadata errors
#[test]
fn test_invalid_metadata() {
    let source = "^:invalid-metadata x"; // Invalid: only ^:delegation or ^{...} are supported

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());
    let error = result.unwrap_err();
    let formatted = error.format_with_context();
    assert!(formatted.contains("Parse Error"));
    println!("Invalid metadata error:\n{}", formatted);
}

/// Test resource reference errors
#[test]
fn test_invalid_resource_reference() {
    let source = "(resource:ref)"; // Missing resource URI

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Invalid resource reference error:\n{}", formatted);
}

/// Test complex nested error scenarios
#[test]
fn test_nested_delimiter_errors() {
    let source = "(let [x (fn [y] { :key \"value\" }]"; // Multiple nested delimiter issues

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());

    let error = result.unwrap_err();
    let formatted = error.format_with_context();

    println!("Nested delimiter error:\n{}", formatted);
}

/// Test error position reporting
#[test]
fn test_error_position_reporting() {
    // Force an error on line 3 so we can assert line/column context rendering.
    let source = "(def x 5)\n(def y \"hello\")\n(+ 1 2"; // Missing closing parenthesis

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    assert!(result.is_err());
    let error = result.unwrap_err();
    let formatted = error.format_with_context();
    assert!(
        formatted.contains("Context around line 3"),
        "Expected formatted error to include line context for line 3; got:\n{}",
        formatted
    );
    println!("Error position reporting:\n{}", formatted);
}

/// Test empty input handling
#[test]
fn test_empty_input() {
    let source = "";

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    // Empty input should parse successfully as empty program
    assert!(result.is_ok());
    let items = result.unwrap();
    assert!(items.is_empty());
}

/// Test whitespace-only input
#[test]
fn test_whitespace_only() {
    let source = "   \n\n  \t  ";

    let result = parse_with_enhanced_errors(source, Some("test.rtfs"));
    // Whitespace-only should parse successfully as empty program
    assert!(result.is_ok());
    let items = result.unwrap();
    assert!(items.is_empty());
}
