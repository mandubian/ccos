// Enhanced parser error reporting for RTFS
// Provides detailed error messages with source location, code snippets, and helpful hints

use crate::error_reporting::{DiagnosticInfo, ErrorHint, SourceSpan};
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
        column: usize,
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

        // Check for delimiter mismatch errors first
        if let Some((mismatch_msg, mismatch_hints)) = Self::detect_delimiter_mismatch(source_code, line, column) {
            message = mismatch_msg;
            hints.extend(mismatch_hints);
            return (message, hints);
        }

        // Check for special form specific errors
        if let Some((special_msg, special_hints)) = Self::detect_special_form_errors(current_line, positives) {
            message = special_msg;
            hints.extend(special_hints);
        }

        // Check for function call syntax errors
        if let Some((func_msg, func_hints)) = Self::detect_function_call_errors(current_line, positives) {
            if message.is_empty() {
                message = func_msg;
            }
            hints.extend(func_hints);
        }

        // Analyze what was expected vs what was found (fallback)
        if message.is_empty() && !positives.is_empty() {
            let expected: Vec<String> = positives.iter().map(|r| format!("{:?}", r)).collect();
            message = format!("Expected one of: {}", expected.join(", "));
        }

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

        // Add context-specific hints using the new helper method
        Self::add_context_specific_hints(&mut hints, current_line);

        (message, hints)
    }

    /// Detect delimiter mismatch errors and provide location of opening delimiters
    fn detect_delimiter_mismatch(source_code: &str, error_line: usize, error_column: usize) -> Option<(String, Vec<ErrorHint>)> {
        let lines: Vec<&str> = source_code.lines().collect();
        if error_line == 0 || error_line > lines.len() {
            return None;
        }

        // Track delimiter stack with positions
        let mut delimiter_stack: Vec<(char, usize, usize)> = Vec::new(); // (delimiter, line, column)
        
        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx + 1;
            
            for (col_idx, ch) in line.chars().enumerate() {
                let col_num = col_idx + 1;
                
                match ch {
                    '(' | '[' | '{' => {
                        delimiter_stack.push((ch, line_num, col_num));
                    }
                    ')' | ']' | '}' => {
                        if let Some((opening, open_line, open_col)) = delimiter_stack.pop() {
                            let expected_closing = match opening {
                                '(' => ')',
                                '[' => ']',
                                '{' => '}',
                                _ => ch,
                            };
                            
                            if ch != expected_closing {
                                let message = format!(
                                    "Mismatched delimiter: found '{}' but expected '{}' to close '{}' from line {}:{}",
                                    ch, expected_closing, opening, open_line, open_col
                                );
                                let hint = ErrorHint::new(&format!(
                                    "The '{}' delimiter opened at line {}:{} was not properly closed",
                                    opening, open_line, open_col
                                )).with_suggestion(&format!(
                                    "Change '{}' to '{}' or add proper closing delimiter",
                                    ch, expected_closing
                                ));
                                return Some((message, vec![hint]));
                            }
                        } else {
                            let message = format!("Unexpected closing delimiter '{}'", ch);
                            let hint = ErrorHint::new("Found closing delimiter without matching opening delimiter")
                                .with_suggestion("Remove the extra delimiter or add a matching opening delimiter");
                            return Some((message, vec![hint]));
                        }
                    }
                    _ => {} // Ignore other characters
                }
                
                // If we've reached the error position and there are unclosed delimiters
                if line_num == error_line && col_num >= error_column && !delimiter_stack.is_empty() {
                    let (opening, open_line, open_col) = delimiter_stack.last().unwrap();
                    let expected_closing = match opening {
                        '(' => ')',
                        '[' => ']',
                        '{' => '}',
                        _ => '?',
                    };
                    let message = format!(
                        "Unclosed delimiter: '{}' opened at line {}:{} was never closed",
                        opening, open_line, open_col
                    );
                    let hint = ErrorHint::new(&format!(
                        "The '{}' delimiter needs to be closed with '{}'",
                        opening, expected_closing
                    )).with_suggestion(&format!(
                        "Add '{}' to close the expression started at line {}:{}",
                        expected_closing, open_line, open_col
                    ));
                    return Some((message, vec![hint]));
                }
            }
        }

        None
    }

    /// Detect special form specific errors (let, if, fn, etc.)
    fn detect_special_form_errors(current_line: &str, positives: &[Rule]) -> Option<(String, Vec<ErrorHint>)> {
        let trimmed = current_line.trim();
        
        // Check for let syntax errors
        if trimmed.starts_with("(let ") && !trimmed.contains("[") {
            let message = "Invalid let syntax: missing binding vector".to_string();
            let hint = ErrorHint::new("let expressions require a binding vector")
                .with_suggestion("Use syntax: (let [binding1 value1 binding2 value2 ...] body-expressions...)");
            return Some((message, vec![hint]));
        }
        
        // Check for if syntax errors
        if trimmed == "(if)" || (trimmed.starts_with("(if ") && trimmed.matches(" ").count() < 2) {
            let message = "Invalid if syntax: missing condition or branches".to_string();
            let mut hints = vec![
                ErrorHint::new("if expressions require at least a condition and then-branch")
                    .with_suggestion("Use syntax: (if condition then-branch) or (if condition then-branch else-branch)")
            ];
            if trimmed == "(if)" {
                hints.push(ErrorHint::new("Empty if expression")
                    .with_suggestion("Add a condition: (if true 'yes' 'no')"));
            }
            return Some((message, hints));
        }
        
        // Check for fn syntax errors
        if trimmed == "(fn)" || (trimmed.starts_with("(fn ") && !trimmed.contains("[")) {
            let message = "Invalid fn syntax: missing parameter list or body".to_string();
            let mut hints = vec![
                ErrorHint::new("fn expressions require a parameter list and body")
                    .with_suggestion("Use syntax: (fn [param1 param2 ...] body-expressions...)")
            ];
            if trimmed == "(fn)" {
                hints.push(ErrorHint::new("Empty fn expression")
                    .with_suggestion("Add parameters and body: (fn [x y] (+ x y))"));
            }
            return Some((message, hints));
        }
        
        // Check for def syntax errors  
        if trimmed == "(def)" || (trimmed.starts_with("(def ") && trimmed.matches(" ").count() < 2) {
            let message = "Invalid def syntax: missing symbol or value".to_string();
            let mut hints = vec![
                ErrorHint::new("def expressions require a symbol and value")
                    .with_suggestion("Use syntax: (def symbol-name value-expression)")
            ];
            if trimmed == "(def)" {
                hints.push(ErrorHint::new("Empty def expression")
                    .with_suggestion("Add symbol and value: (def my-var 42)"));
            }
            return Some((message, hints));
        }
        
        // Check for defn syntax errors
        if trimmed == "(defn)" || (trimmed.starts_with("(defn ") && !trimmed.contains("[")) {
            let message = "Invalid defn syntax: missing name, parameters, or body".to_string();
            let mut hints = vec![
                ErrorHint::new("defn expressions require a name, parameter list, and body")
                    .with_suggestion("Use syntax: (defn function-name [param1 param2 ...] body-expressions...)")
            ];
            if trimmed == "(defn)" {
                hints.push(ErrorHint::new("Empty defn expression")
                    .with_suggestion("Add name, parameters and body: (defn add [x y] (+ x y))"));
            }
            return Some((message, hints));
        }

        None
    }

    /// Detect function call syntax errors and provide suggestions
    fn detect_function_call_errors(current_line: &str, positives: &[Rule]) -> Option<(String, Vec<ErrorHint>)> {
        let trimmed = current_line.trim();
        
        // Check for empty function call
        if trimmed == "()" {
            let message = "Empty list: not a valid function call".to_string();
            let hint = ErrorHint::new("Function calls require a function name")
                .with_suggestion("Try: (function-name arg1 arg2 ...) or use [] for empty vector, {} for empty map");
            return Some((message, vec![hint]));
        }
        
        // Check for incomplete function calls (unclosed parentheses)
        if trimmed.starts_with("(") && !trimmed.ends_with(")") && trimmed.matches('(').count() > trimmed.matches(')').count() {
            let message = "Incomplete function call: missing closing parenthesis".to_string();
            let hint = ErrorHint::new("Function calls must be properly closed")
                .with_suggestion("Add ')' to complete the function call");
            return Some((message, vec![hint]));
        }
        
        // Check for potential function call issues based on expected rules
        if positives.contains(&Rule::expression) || positives.contains(&Rule::list) {
            // If we expect an expression or list, provide function call guidance
            if trimmed.starts_with("(") && trimmed.len() > 1 && !trimmed.contains(" ") {
                let hint = ErrorHint::new("If this is intended as a function call, add arguments")
                    .with_suggestion("Function call syntax: (function-name arg1 arg2 ...)");
                return Some((String::new(), vec![hint])); // Empty message to not override main message
            }
        }

        None
    }

    /// Enhanced context analysis with better error categorization
    fn add_context_specific_hints(hints: &mut Vec<ErrorHint>, current_line: &str) {
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
    }

    /// Format the error with source code context
    pub fn format_with_context(&self) -> String {
        let mut output = String::new();

        // Error header
        output.push_str(&format!("❌ Parse Error: {}\n", self.message));

        if let Some(file_path) = &self.file_path {
            output.push_str(&format!("📁 File: {}\n", file_path));
        }

        // Enhanced source code context with multiple lines
        if let Some(span) = &self.diagnostic.primary_span {
            if let Some(source_text) = &span.source_text {
                let lines: Vec<&str> = source_text.lines().collect();
                let error_line = span.start_line;
                let context_lines = 3; // Show 3 lines before and after for better context

                output.push_str(&format!("\n📍 Context around line {}:\n", error_line));

                // Show context lines with enhanced formatting
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
                        
                        // Add line number with consistent formatting
                        output.push_str(&format!("{:4} {}{}\n", line_num, prefix, line_content));

                        // Enhanced pointer with column indicator on the error line
                        if line_num == error_line {
                            let column = span.start_column;
                            if column > 0 && column <= line_content.len() + 1 {
                                let mut pointer_line = String::new();
                                pointer_line.push_str("     "); // Align with line content
                                
                                // Add spaces to align with error column
                                for i in 1..column {
                                    if i <= line_content.len() {
                                        let ch = line_content.chars().nth(i - 1).unwrap_or(' ');
                                        if ch == '\t' {
                                            pointer_line.push('\t');
                                        } else {
                                            pointer_line.push(' ');
                                        }
                                    } else {
                                        pointer_line.push(' ');
                                    }
                                }
                                
                                pointer_line.push_str("^");
                                
                                // Add range indicator if error spans multiple columns
                                if span.end_column > span.start_column && span.start_line == span.end_line {
                                    let range_len = span.end_column - span.start_column;
                                    for _ in 1..range_len {
                                        pointer_line.push('~');
                                    }
                                }
                                
                                output.push_str(&format!("{}\n", pointer_line));
                                output.push_str(&format!("     Here at column {}\n", column));
                            }
                        }
                    }
                }
                
                // Add delimiter stack information if available
                if self.message.contains("delimiter") || self.message.contains("Unclosed") || self.message.contains("Mismatched") {
                    output.push_str("\n🔍 Delimiter Analysis:\n");
                    output.push_str("   Check that all opening delimiters have matching closing delimiters:\n");
                    output.push_str("   • Parentheses: ( ... )\n");
                    output.push_str("   • Brackets: [ ... ]\n");
                    output.push_str("   • Braces: { ... }\n");
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

    /// Create a customized parser error reporter
    pub fn with_config(use_colors: bool, show_source_context: bool, max_context_lines: usize) -> Self {
        Self {
            use_colors,
            show_source_context,
            max_context_lines,
        }
    }

    /// Enable or disable colored output
    pub fn with_colors(mut self, use_colors: bool) -> Self {
        self.use_colors = use_colors;
        self
    }

    /// Enable or disable source context display
    pub fn with_source_context(mut self, show_source_context: bool) -> Self {
        self.show_source_context = show_source_context;
        self
    }

    /// Set maximum number of context lines to show
    pub fn with_max_context_lines(mut self, max_lines: usize) -> Self {
        self.max_context_lines = max_lines;
        self
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

        // Check that the error has a non-empty message and diagnostic hints
        assert!(!error.message.is_empty());
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
