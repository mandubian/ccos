// Enhanced error reporting system for the RTFS compiler
// Provides source location information, code snippets, and helpful hints

use crate::ast::Symbol;
use std::fmt;
use validator::ValidationErrors;

/// Source span representing a range in the source code
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpan {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub file_path: Option<String>,
    pub source_text: Option<String>, // The actual source code text
}

impl SourceSpan {
    pub fn new(start_line: usize, start_column: usize, end_line: usize, end_column: usize) -> Self {
        Self {
            start_line,
            start_column,
            end_line,
            end_column,
            file_path: None,
            source_text: None,
        }
    }

    pub fn with_file(mut self, file_path: String) -> Self {
        self.file_path = Some(file_path);
        self
    }

    pub fn with_source_text(mut self, source_text: String) -> Self {
        self.source_text = Some(source_text);
        self
    }

    pub fn single_point(line: usize, column: usize) -> Self {
        Self::new(line, column, line, column)
    }

    /// Creates a new zero-length span at the end of the current span.
    /// Useful for indicating a position immediately after a token.
    pub fn end_as_start(&self) -> Self {
        Self {
            start_line: self.end_line,
            start_column: self.end_column,
            end_line: self.end_line,
            end_column: self.end_column,
            file_path: self.file_path.clone(),
            // Source text for a zero-length span is typically empty or a specific marker
            source_text: Some("".to_string()),
        }
    }
}

/// Error severity levels
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Contextual hint for error resolution
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorHint {
    pub message: String,
    pub suggested_fix: Option<String>,
    pub related_span: Option<SourceSpan>,
}

impl ErrorHint {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            suggested_fix: None,
            related_span: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: &str) -> Self {
        self.suggested_fix = Some(suggestion.to_string());
        self
    }

    pub fn with_related_span(mut self, span: SourceSpan) -> Self {
        self.related_span = Some(span);
        self
    }
}

/// Enhanced diagnostic information for compile-time and runtime errors
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticInfo {
    pub error_code: String,
    pub severity: ErrorSeverity,
    pub primary_message: String,
    pub primary_span: Option<SourceSpan>,
    pub secondary_spans: Vec<(SourceSpan, String)>, // Additional spans with labels
    pub hints: Vec<ErrorHint>,
    pub notes: Vec<String>,
    pub caused_by: Option<Box<DiagnosticInfo>>, // Error chaining
}

impl DiagnosticInfo {
    pub fn new(error_code: &str, severity: ErrorSeverity, message: &str) -> Self {
        Self {
            error_code: error_code.to_string(),
            severity,
            primary_message: message.to_string(),
            primary_span: None,
            secondary_spans: Vec::new(),
            hints: Vec::new(),
            notes: Vec::new(),
            caused_by: None,
        }
    }

    pub fn error(error_code: &str, message: &str) -> Self {
        Self::new(error_code, ErrorSeverity::Error, message)
    }

    pub fn warning(error_code: &str, message: &str) -> Self {
        Self::new(error_code, ErrorSeverity::Warning, message)
    }

    pub fn with_primary_span(mut self, span: SourceSpan) -> Self {
        self.primary_span = Some(span);
        self
    }

    pub fn with_secondary_span(mut self, span: SourceSpan, label: &str) -> Self {
        self.secondary_spans.push((span, label.to_string()));
        self
    }

    pub fn with_hint(mut self, hint: ErrorHint) -> Self {
        self.hints.push(hint);
        self
    }

    pub fn with_note(mut self, note: &str) -> Self {
        self.notes.push(note.to_string());
        self
    }

    pub fn caused_by(mut self, cause: DiagnosticInfo) -> Self {
        self.caused_by = Some(Box::new(cause));
        self
    }
}

/// Enhanced runtime error with diagnostic information
#[derive(Debug, Clone, PartialEq)]
pub enum EnhancedRuntimeError {
    /// Type errors with enhanced context
    TypeError {
        expected: String,
        actual: String,
        operation: String,
        diagnostic: DiagnosticInfo,
    },

    /// Undefined symbol with suggestions
    UndefinedSymbol {
        symbol: Symbol,
        diagnostic: DiagnosticInfo,
    },

    /// Arity mismatch with clear parameter information
    ArityMismatch {
        function: String,
        expected_min: usize,
        expected_max: Option<usize>,
        actual: usize,
        diagnostic: DiagnosticInfo,
    },

    /// Parse errors with syntax highlighting
    ParseError {
        message: String,
        diagnostic: DiagnosticInfo,
    },

    /// IR conversion errors
    IrConversionError {
        message: String,
        diagnostic: DiagnosticInfo,
    },

    /// Generic runtime error with diagnostic
    RuntimeError {
        message: String,
        diagnostic: DiagnosticInfo,
    },
}

impl EnhancedRuntimeError {
    pub fn diagnostic(&self) -> &DiagnosticInfo {
        match self {
            Self::TypeError { diagnostic, .. } => diagnostic,
            Self::UndefinedSymbol { diagnostic, .. } => diagnostic,
            Self::ArityMismatch { diagnostic, .. } => diagnostic,
            Self::ParseError { diagnostic, .. } => diagnostic,
            Self::IrConversionError { diagnostic, .. } => diagnostic,
            Self::RuntimeError { diagnostic, .. } => diagnostic,
        }
    }

    /// Create undefined symbol error with smart suggestions
    pub fn undefined_symbol(
        symbol: &Symbol,
        span: SourceSpan,
        available_symbols: &[String],
    ) -> Self {
        let suggestions = find_similar_symbols(&symbol.0, available_symbols);

        let mut diagnostic =
            DiagnosticInfo::error("E001", &format!("Undefined symbol `{}`", symbol.0))
                .with_primary_span(span);

        if !suggestions.is_empty() {
            let suggestion_text = if suggestions.len() == 1 {
                format!("Did you mean `{}`?", suggestions[0])
            } else {
                format!("Did you mean one of: {}?", suggestions.join(", "))
            };

            diagnostic = diagnostic
                .with_hint(ErrorHint::new(&suggestion_text).with_suggestion(&suggestions[0]));
        }

        Self::UndefinedSymbol {
            symbol: symbol.clone(),
            diagnostic,
        }
    }

    /// Create type error with enhanced context
    pub fn type_error(expected: &str, actual: &str, operation: &str, span: SourceSpan) -> Self {
        let diagnostic = DiagnosticInfo::error(
            "E002",
            &format!(
                "Type mismatch in {}: expected {}, found {}",
                operation, expected, actual
            ),
        )
        .with_primary_span(span)
        .with_hint(ErrorHint::new(&format!(
            "The operation `{}` requires a value of type `{}`, but got `{}`",
            operation, expected, actual
        )));

        Self::TypeError {
            expected: expected.to_string(),
            actual: actual.to_string(),
            operation: operation.to_string(),
            diagnostic,
        }
    }

    /// Create arity mismatch error with parameter details
    pub fn arity_mismatch(
        function: &str,
        expected_min: usize,
        expected_max: Option<usize>,
        actual: usize,
        span: SourceSpan,
    ) -> Self {
        let expected_str = match expected_max {
            Some(max) if max == expected_min => expected_min.to_string(),
            Some(max) => format!("{}-{}", expected_min, max),
            None => format!("{}+", expected_min),
        };

        let diagnostic = DiagnosticInfo::error(
            "E003",
            &format!(
                "Function `{}` expects {} arguments, but {} were provided",
                function, expected_str, actual
            ),
        )
        .with_primary_span(span)
        .with_hint(ErrorHint::new(&format!(
            "Adjust the number of arguments to match the function signature"
        )));

        Self::ArityMismatch {
            function: function.to_string(),
            expected_min,
            expected_max,
            actual,
            diagnostic,
        }
    }
}

/// Find similar symbols using Levenshtein distance
fn find_similar_symbols(target: &str, available: &[String]) -> Vec<String> {
    let mut candidates: Vec<_> = available
        .iter()
        .map(|s| (s, levenshtein_distance(target, s)))
        .filter(|(_, dist)| *dist <= 3 && *dist < target.len()) // Only suggest if reasonable distance
        .collect();

    candidates.sort_by_key(|(_, dist)| *dist);

    candidates
        .into_iter()
        .take(3) // Limit to top 3 suggestions
        .map(|(s, _)| s.clone())
        .collect()
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    // Initialize first row and column
    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(
                    matrix[i - 1][j] + 1, // deletion
                    matrix[i][j - 1] + 1, // insertion
                ),
                matrix[i - 1][j - 1] + cost, // substitution
            );
        }
    }

    matrix[a_len][b_len]
}

/// Pretty-print diagnostic information
pub struct DiagnosticFormatter {
    pub use_colors: bool,
    pub show_line_numbers: bool,
    pub context_lines: usize,
}

impl Default for DiagnosticFormatter {
    fn default() -> Self {
        Self {
            use_colors: true,
            show_line_numbers: true,
            context_lines: 2,
        }
    }
}

impl DiagnosticFormatter {
    pub fn format_diagnostic(&self, diagnostic: &DiagnosticInfo) -> String {
        let mut output = String::new();

        // Header with error code and severity
        let severity_str = match diagnostic.severity {
            ErrorSeverity::Error => "error",
            ErrorSeverity::Warning => "warning",
            ErrorSeverity::Info => "info",
            ErrorSeverity::Hint => "hint",
        };

        output.push_str(&format!(
            "{}: {}: {}\n",
            severity_str, diagnostic.error_code, diagnostic.primary_message
        ));

        // Primary span with source code
        if let Some(ref span) = diagnostic.primary_span {
            output.push_str(&self.format_source_span(span, ""));
        }

        // Secondary spans
        for (span, label) in &diagnostic.secondary_spans {
            output.push_str(&self.format_source_span(span, label));
        }

        // Hints
        for hint in &diagnostic.hints {
            output.push_str(&format!("  = help: {}\n", hint.message));
            if let Some(ref suggestion) = hint.suggested_fix {
                output.push_str(&format!("  = suggestion: {}\n", suggestion));
            }
        }

        // Notes
        for note in &diagnostic.notes {
            output.push_str(&format!("  = note: {}\n", note));
        }

        // Caused by (error chaining)
        if let Some(ref cause) = diagnostic.caused_by {
            output.push_str("\nCaused by:\n");
            output.push_str(&self.format_diagnostic(cause));
        }

        output
    }

    fn format_source_span(&self, span: &SourceSpan, label: &str) -> String {
        let mut output = String::new();

        // File location
        if let Some(ref file) = span.file_path {
            output.push_str(&format!(
                "  --> {}:{}:{}\n",
                file, span.start_line, span.start_column
            ));
        } else {
            output.push_str(&format!(
                "  --> line {}:{}\n",
                span.start_line, span.start_column
            ));
        }

        // Source code snippet
        if let Some(ref source) = span.source_text {
            output.push_str(&self.format_source_snippet(source, span, label));
        }

        output
    }

    fn format_source_snippet(&self, source: &str, span: &SourceSpan, label: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let mut output = String::new();

        let start_line = span.start_line.saturating_sub(1); // Convert to 0-based
        let end_line = span.end_line.saturating_sub(1);

        let context_start = start_line.saturating_sub(self.context_lines);
        let context_end = std::cmp::min(end_line + self.context_lines, lines.len());

        // Line number width for formatting
        let line_num_width = context_end.to_string().len();

        for (i, line) in lines
            .iter()
            .enumerate()
            .take(context_end)
            .skip(context_start)
        {
            let line_num = i + 1;
            let is_error_line = i >= start_line && i <= end_line;

            if self.show_line_numbers {
                if is_error_line {
                    output.push_str(&format!(
                        "{:width$} | {}\n",
                        line_num,
                        line,
                        width = line_num_width
                    ));

                    // Add caret indicators
                    let start_col = if i == start_line {
                        span.start_column.saturating_sub(1)
                    } else {
                        0
                    };
                    let end_col = if i == end_line {
                        span.end_column
                    } else {
                        line.len()
                    };

                    output.push_str(&format!(
                        "{:width$} | {}{}{}\n",
                        "",
                        " ".repeat(start_col),
                        "^".repeat(std::cmp::max(1, end_col - start_col)),
                        if !label.is_empty() {
                            format!(" {}", label)
                        } else {
                            String::new()
                        },
                        width = line_num_width
                    ));
                } else {
                    output.push_str(&format!(
                        "{:width$} | {}\n",
                        line_num,
                        line,
                        width = line_num_width
                    ));
                }
            } else {
                output.push_str(&format!("{}\n", line));
                if is_error_line && !label.is_empty() {
                    output.push_str(&format!("   {}\n", label));
                }
            }
        }

        output
    }
}

impl fmt::Display for EnhancedRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatter = DiagnosticFormatter::default();
        write!(f, "{}", formatter.format_diagnostic(self.diagnostic()))
    }
}

/// Convert from legacy RuntimeError to EnhancedRuntimeError
impl From<crate::runtime::error::RuntimeError> for EnhancedRuntimeError {
    fn from(error: crate::runtime::error::RuntimeError) -> Self {
        match error {
            crate::runtime::error::RuntimeError::TypeError {
                expected,
                actual,
                operation,
            } => {
                // Create a diagnostic without span for legacy compatibility
                let diagnostic = DiagnosticInfo::error(
                    "E002",
                    &format!(
                        "Type mismatch in {}: expected {}, got {}",
                        operation, expected, actual
                    ),
                );

                Self::TypeError {
                    expected,
                    actual,
                    operation,
                    diagnostic,
                }
            }
            crate::runtime::error::RuntimeError::UndefinedSymbol(symbol) => {
                let diagnostic =
                    DiagnosticInfo::error("E001", &format!("Undefined symbol: {}", symbol.0));

                Self::UndefinedSymbol { symbol, diagnostic }
            }
            _ => {
                let diagnostic = DiagnosticInfo::error("E999", &error.to_string());
                Self::RuntimeError {
                    message: error.to_string(),
                    diagnostic,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("test", "test"), 0);
        assert_eq!(levenshtein_distance("test", "tests"), 1);
        assert_eq!(levenshtein_distance("map", "mpa"), 2);
        assert_eq!(levenshtein_distance("hello", "world"), 4);
    }

    #[test]
    fn test_find_similar_symbols() {
        let symbols = vec![
            "map".to_string(),
            "reduce".to_string(),
            "filter".to_string(),
            "apply".to_string(),
        ];

        let suggestions = find_similar_symbols("mpa", &symbols);
        assert!(suggestions.contains(&"map".to_string()));

        let suggestions = find_similar_symbols("fiter", &symbols);
        assert!(suggestions.contains(&"filter".to_string()));
    }

    #[test]
    fn test_diagnostic_formatting() {
        let span = SourceSpan::new(1, 5, 1, 8).with_source_text("(let [x 10] (+ x y))".to_string());

        let diagnostic = DiagnosticInfo::error("E001", "Undefined symbol `y`")
            .with_primary_span(span)
            .with_hint(ErrorHint::new("Did you mean `x`?").with_suggestion("x"));

        let formatter = DiagnosticFormatter::default();
        let output = formatter.format_diagnostic(&diagnostic);

        assert!(output.contains("error: E001: Undefined symbol `y`"));
        assert!(output.contains("help: Did you mean `x`?"));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    SchemaError {
        type_name: String,
        errors: ValidationErrors,
    },
    Custom(String),
}
