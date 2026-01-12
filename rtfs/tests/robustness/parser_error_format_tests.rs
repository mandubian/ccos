// Regression tests for ParserError formatting
// Goal: keep error structure stable (header, file, context, pointer, delimiter analysis).

use rtfs::parser::parse_with_enhanced_errors;

#[test]
fn test_formatted_error_has_header_file_and_context() {
    // Force an error on line 3.
    let source = "(def x 5)\n(def y \"hello\")\n(+ 1 2";
    let err = parse_with_enhanced_errors(source, Some("format.rtfs")).unwrap_err();
    let formatted = err.format_with_context();

    assert!(
        formatted.starts_with("❌ Parse Error:"),
        "missing header:\n{}",
        formatted
    );
    assert!(
        formatted.contains("📁 File: format.rtfs"),
        "missing file path:\n{}",
        formatted
    );
    assert!(
        formatted.contains("📍 Context around line 3:"),
        "missing context header:\n{}",
        formatted
    );
    assert!(
        formatted.contains("Here at column"),
        "missing column indicator:\n{}",
        formatted
    );
}

#[test]
fn test_delimiter_mismatch_includes_delimiter_analysis_section() {
    let source = "[1 2 3)";
    let err = parse_with_enhanced_errors(source, Some("format.rtfs")).unwrap_err();
    let formatted = err.format_with_context();

    assert!(
        formatted.contains("Mismatched delimiter")
            || formatted.contains("Unclosed delimiter")
            || formatted.contains("Unexpected closing delimiter"),
        "expected delimiter-related message:\n{}",
        formatted
    );
    assert!(
        formatted.contains("🔍 Delimiter Analysis:"),
        "missing delimiter analysis section:\n{}",
        formatted
    );
    assert!(formatted.contains("Parentheses: ( ... )"));
    assert!(formatted.contains("Brackets: [ ... ]"));
    assert!(formatted.contains("Braces: { ... }"));
}
