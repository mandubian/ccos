// Enhanced parser error reporting for RTFS
// Provides detailed error messages with source location, code snippets, and helpful hints

use crate::error_reporting::{DiagnosticInfo, ErrorHint, ErrorSeverity, SourceSpan};
use crate::parser::Rule;
use pest::error::{Error as PestError, ErrorVariant, InputLocation};
use pest::Position;
use std::fmt;

/// Enhanced parser error with detailed diagnostic information
#[derive(Debug, Clone)]
pub struct ParserError {
    pub message: String,
    pub diagnostic: DiagnosticInfo,
    pub source_code: String,
    pub file_path: Option<String>,
}

impl ParserError {
    /// Create a new parser error from a Pest error
    pub fn from_pest_error(
        pest_error: PestError<Rule>,
        source_code: String,
        file_path: Option<String>,
    ) -> Self {
        let (message, diagnostic) = Self::analyze_pest_error(&pest_error, &source_code);

        Self {
            message,
            diagnostic,
            source_code,
            file_path,
        }
    }

    /// Analyze a Pest error and create detailed diagnostic information
    fn analyze_pest_error(
        pest_error: &PestError<Rule>,
        source_code: &str,
    ) -> (String, DiagnosticInfo) {
        let location = pest_error.location.clone();
        let (line, column) = match location {
            InputLocation::Pos(pos) => {
                let position = Position::new(source_code, pos).unwrap();
                position.line_col()
            }
            InputLocation::Span((start, _)) => {
                let position = Position::new(source_code, start).unwrap();
                position.line_col()
            }
        };

        // Create source span
        let span = SourceSpan::single_point(line, column).with_source_text(source_code.to_string());

        // Analyze the error variant and create appropriate diagnostic
        match &pest_error.variant {
            ErrorVariant::ParsingError {
                positives,
                negatives,
            } => {
                let (message, hints) =
                    Self::analyze_parsing_error(positives, negatives, source_code, line, column);
                let mut diagnostic =
                    DiagnosticInfo::error("E001", &message).with_primary_span(span);
                for hint in hints {
                    diagnostic = diagnostic.with_hint(hint);
                }
                (message, diagnostic)
            }
            ErrorVariant::CustomError { message } => {
                let diagnostic = DiagnosticInfo::error("E002", message).with_primary_span(span);
                (message.clone(), diagnostic)
            }
        }
    }

    /// Analyze parsing errors and provide specific hints
    fn analyze_parsing_error(
        positives: &[Rule],
        _negatives: &[Rule],
        source_code: &str,
        line: usize,
        _column: usize,
    ) -> (String, Vec<ErrorHint>) {
        let mut hints = Vec::new();
        let mut message = String::new();

        // Get the line of code where the error occurred
        let lines: Vec<&str> = source_code.lines().collect();
        let current_line = if line <= lines.len() {
            lines[line - 1]
        } else {
            ""
        };

        // Analyze what was expected vs what was found
        if !positives.is_empty() {
            let expected: Vec<String> = positives.iter().map(|r| format!("{:?}", r)).collect();
            message = format!("Expected one of: {}", expected.join(", "));
            // Provide specific hints based on what was expected
            for positive in positives {
                match positive {
                    Rule::map_key => {
                        hints.push(
                            ErrorHint::new("Map keys must be keywords, strings, or integers")
                                .with_suggestion(
                                    "Try using :keyword, \"string\", or 123 as map keys",
                                ),
                        );
                    }
                    Rule::expression => {
                        hints.push(
                            ErrorHint::new("Expected an expression here")
                                .with_suggestion("Try adding a value, variable, or function call"),
                        );
                    }
                    Rule::symbol => {
                        hints.push(ErrorHint::new("Expected a symbol (identifier) here")
                            .with_suggestion("Try using a valid identifier like 'my-function' or 'variable-name'"));
                    }
                    Rule::keyword => {
                        hints.push(ErrorHint::new("Expected a keyword here").with_suggestion(
                            "Keywords start with ':' like :keyword or :my.keyword",
                        ));
                    }
                    Rule::string => {
                        hints.push(
                            ErrorHint::new("Expected a string literal here").with_suggestion(
                                "Try wrapping text in quotes like \"hello world\"",
                            ),
                        );
                    }
                    Rule::integer => {
                        hints.push(
                            ErrorHint::new("Expected an integer here")
                                .with_suggestion("Try using a number like 42 or -17"),
                        );
                    }
                    Rule::list => {
                        hints.push(ErrorHint::new("Expected a list here").with_suggestion(
                            "Lists are enclosed in parentheses like (function arg1 arg2)",
                        ));
                    }
                    Rule::vector => {
                        hints.push(ErrorHint::new("Expected a vector here").with_suggestion(
                            "Vectors are enclosed in square brackets like [1 2 3]",
                        ));
                    }
                    Rule::map => {
                        hints.push(
                            ErrorHint::new("Expected a map here").with_suggestion(
                                "Maps are enclosed in braces like {:key \"value\"}",
                            ),
                        );
                    }
                    Rule::module_definition => {
                        hints.push(
                            ErrorHint::new("Expected a module definition")
                                .with_suggestion("Try: (module my-module ...)"),
                        );
                    }
                    _ => {
                        hints.push(
                            ErrorHint::new(&format!("Expected '{:?}' here", positive))
                                .with_suggestion(&format!(
                                    "Try adding '{:?}' at this position",
                                    positive
                                )),
                        );
                    }
                }
            }
        }

        // Add context-specific hints based on the current line
        if !current_line.is_empty() {
            let trimmed = current_line.trim();
            if trimmed.starts_with("module") && !trimmed.starts_with("(") {
                hints.push(
                    ErrorHint::new("Module definitions must be enclosed in parentheses")
                        .with_suggestion("Try: (module my-module ...)"),
                );
            } else if trimmed.starts_with("//") {
                hints.push(
                    ErrorHint::new("RTFS uses semicolon (;) for comments, not double slash (//)")
                        .with_suggestion("Change // to ; for comments"),
                );
            }
        }

        // Add general RTFS syntax hints
        hints.push(
            ErrorHint::new("RTFS 2.0 supports both expressions and object definitions")
                .with_suggestion(
                    "You can mix RTFS 1.0 expressions with RTFS 2.0 objects in the same file",
                ),
        );

        (message, hints)
    }

    /// Format the error with source code context
    pub fn format_with_context(&self) -> String {
        let mut output = String::new();

        // Error header
        output.push_str(&format!("❌ Parse Error: {}\n", self.message));

        if let Some(file_path) = &self.file_path {
            output.push_str(&format!("📁 File: {}\n", file_path));
        }

        // Source code context with multiple lines
        if let Some(span) = &self.diagnostic.primary_span {
            if let Some(source_text) = &span.source_text {
                let lines: Vec<&str> = source_text.lines().collect();
                let error_line = span.start_line;
                let context_lines = 2; // Show 2 lines before and after

                output.push_str(&format!("\n📍 Context around line {}:\n", error_line));

                // Show context lines before the error
                let start_line = if error_line > context_lines {
                    error_line - context_lines
                } else {
                    1
                };

                let end_line = if error_line + context_lines <= lines.len() {
                    error_line + context_lines
                } else {
                    lines.len()
                };

                for line_num in start_line..=end_line {
                    if line_num <= lines.len() {
                        let line_content = lines[line_num - 1];
                        let prefix = if line_num == error_line {
                            "❌ "
                        } else {
                            "   "
                        };
                        output.push_str(&format!("{:4} {}{}\n", line_num, prefix, line_content));

                        // Add pointer to the error location on the error line
                        if line_num == error_line {
                            let column = span.start_column;
                            if column <= line_content.len() {
                                let pointer = " ".repeat(column - 1) + "^";
                                output.push_str(&format!("     {}\n", pointer));
                            }
                        }
                    }
                }
            }
        }

        // Hints
        if !self.diagnostic.hints.is_empty() {
            output.push_str("\n💡 Hints:\n");
            for (i, hint) in self.diagnostic.hints.iter().enumerate() {
                output.push_str(&format!("   {}. {}\n", i + 1, hint.message));
                if let Some(suggestion) = &hint.suggested_fix {
                    output.push_str(&format!("      → {}\n", suggestion));
                }
            }
        }

        // Notes
        if !self.diagnostic.notes.is_empty() {
            output.push_str("\n📝 Notes:\n");
            for note in &self.diagnostic.notes {
                output.push_str(&format!("   • {}\n", note));
            }
        }

        output
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_with_context())
    }
}

impl std::error::Error for ParserError {}

/// Parser error reporter that provides enhanced error messages
pub struct ParserErrorReporter {
    pub use_colors: bool,
    pub show_source_context: bool,
    pub max_context_lines: usize,
}

impl Default for ParserErrorReporter {
    fn default() -> Self {
        Self {
            use_colors: true,
            show_source_context: true,
            max_context_lines: 3,
        }
    }
}

impl ParserErrorReporter {
    /// Create a new parser error reporter
    pub fn new() -> Self {
        Self::default()
    }

    /// Report a parsing error with enhanced context
    pub fn report_error(
        &self,
        pest_error: PestError<Rule>,
        source_code: &str,
        file_path: Option<&str>,
    ) -> ParserError {
        ParserError::from_pest_error(
            pest_error,
            source_code.to_string(),
            file_path.map(|s| s.to_string()),
        )
    }

    /// Format multiple parsing errors
    pub fn format_errors(&self, errors: &[ParserError]) -> String {
        let mut output = String::new();

        for (i, error) in errors.iter().enumerate() {
            if i > 0 {
                output.push_str("\n");
            }
            output.push_str(&error.format_with_context());
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pest::error::ErrorVariant;

    #[test]
    fn test_parser_error_creation() {
        let source_code = "intent my-intent\n  name: test-intent\n}";
        let pest_error = PestError::new_from_pos(
            ErrorVariant::ParsingError {
                positives: vec![Rule::map],
                negatives: vec![],
            },
            pest::Position::new(source_code, 0).unwrap(),
        );

        let error = ParserError::from_pest_error(
            pest_error,
            source_code.to_string(),
            Some("test.rtfs".to_string()),
        );

        assert!(error.message.contains("Expected"));
        assert!(!error.diagnostic.hints.is_empty());
    }

    #[test]
    fn test_comment_syntax_hint() {
        let source_code = "// This is a comment\nlet x = 5";
        let pest_error = PestError::new_from_pos(
            ErrorVariant::ParsingError {
                positives: vec![Rule::expression],
                negatives: vec![],
            },
            pest::Position::new(source_code, 0).unwrap(),
        );

        let error = ParserError::from_pest_error(pest_error, source_code.to_string(), None);

        let has_comment_hint = error
            .diagnostic
            .hints
            .iter()
            .any(|hint| hint.message.contains("semicolon"));

        assert!(has_comment_hint);
    }
}
