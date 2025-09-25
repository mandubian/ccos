use crate::error_reporting::{
    DiagnosticInfo, ErrorHint, ErrorSeverity, SourceSpan, ValidationError,
};
use pest::iterators::Pair;

// Helper function to convert pest span to our SourceSpan
pub fn pest_span_to_source_span(span: pest::Span) -> SourceSpan {
    let (start_line, start_column) = span.start_pos().line_col();
    let (end_line, end_column) = span.end_pos().line_col();
    SourceSpan::new(start_line, start_column, end_line, end_column)
        .with_source_text(span.as_str().to_string())
}

// Helper function to create SourceSpan from a Pair
pub fn pair_to_source_span(pair: &Pair<super::Rule>) -> SourceSpan {
    pest_span_to_source_span(pair.as_span())
}

// Helper functions to create errors with proper spans
pub fn missing_token_error(token: &str, pair: &Pair<super::Rule>) -> PestParseError {
    PestParseError::MissingToken {
        token: token.to_string(),
        span: Some(pair_to_source_span(pair)),
    }
}

pub fn invalid_input_error(message: &str, pair: &Pair<super::Rule>) -> PestParseError {
    PestParseError::InvalidInput {
        message: message.to_string(),
        span: Some(pair_to_source_span(pair)),
    }
}

pub fn invalid_literal_error(message: &str, pair: &Pair<super::Rule>) -> PestParseError {
    PestParseError::InvalidLiteral {
        message: message.to_string(),
        span: Some(pair_to_source_span(pair)),
    }
}

// Create a default span for when we don't have access to the original pair
pub fn default_source_span() -> SourceSpan {
    SourceSpan::new(1, 1, 1, 1).with_source_text("(location unavailable)".to_string())
}

// Define a custom error type for parsing
#[derive(Debug)]
pub enum PestParseError {
    UnexpectedRule {
        expected: String,
        found: String,
        rule_text: String,
        span: Option<SourceSpan>,
    },
    MissingToken {
        token: String,
        span: Option<SourceSpan>,
    },
    InvalidInput {
        message: String,
        span: Option<SourceSpan>,
    },
    UnsupportedRule {
        rule: String,
        span: Option<SourceSpan>,
    },
    InvalidLiteral {
        message: String,
        span: Option<SourceSpan>,
    },
    InvalidEscapeSequence {
        sequence: String,
        span: Option<SourceSpan>,
    },
    CustomError {
        message: String,
        span: Option<SourceSpan>,
    },
    ValidationError(ValidationError),
    PestError(pest::error::Error<super::Rule>),
}

impl From<pest::error::Error<super::Rule>> for PestParseError {
    fn from(err: pest::error::Error<super::Rule>) -> Self {
        PestParseError::PestError(err)
    }
}

impl PestParseError {
    /// Convert the parse error to a diagnostic info for enhanced error reporting
    pub fn to_diagnostic(&self) -> DiagnosticInfo {
        match self {
            PestParseError::UnexpectedRule {
                expected,
                found,
                rule_text,
                span,
            } => DiagnosticInfo {
                error_code: "P001".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: format!("Expected {}, found {}", expected, found),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new(&format!(
                    "Expected a {} expression here",
                    expected
                ))],
                notes: vec![format!("Rule text: {}", rule_text)],
                caused_by: None,
            },
            PestParseError::MissingToken { token, span } => DiagnosticInfo {
                error_code: "P002".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: format!("Missing required token: {}", token),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new(&format!("Add the missing {} token", token))],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::InvalidInput { message, span } => DiagnosticInfo {
                error_code: "P003".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: message.clone(),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new("Check the syntax of your input")],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::UnsupportedRule { rule, span } => DiagnosticInfo {
                error_code: "P004".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: format!("Unsupported rule: {}", rule),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new(&format!(
                    "The {} construct is not yet supported",
                    rule
                ))],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::InvalidLiteral { message, span } => DiagnosticInfo {
                error_code: "P005".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: message.clone(),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new("Check the literal syntax")],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::InvalidEscapeSequence { sequence, span } => DiagnosticInfo {
                error_code: "P006".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: format!("Invalid escape sequence: {}", sequence),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![ErrorHint::new(
                    "Use valid escape sequences like \\n, \\t, \\r, \\\\, or \\\"",
                )],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::CustomError { message, span } => DiagnosticInfo {
                error_code: "P007".to_string(),
                severity: ErrorSeverity::Error,
                primary_message: message.clone(),
                primary_span: span.clone(),
                secondary_spans: vec![],
                hints: vec![],
                notes: vec![],
                caused_by: None,
            },
            PestParseError::ValidationError(validation_err) => match validation_err {
                ValidationError::SchemaError { type_name, errors } => DiagnosticInfo {
                    error_code: "V001".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Schema validation failed for {}", type_name),
                    primary_span: None, // Validation errors don't have a direct span yet
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new(
                        "Ensure the object properties match the schema.",
                    )],
                    notes: vec![format!("{:?}", errors)],
                    caused_by: None,
                },
                ValidationError::Custom(message) => DiagnosticInfo {
                    error_code: "V002".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: message.clone(),
                    primary_span: None,
                    secondary_spans: vec![],
                    hints: vec![],
                    notes: vec![],
                    caused_by: None,
                },
            },
            PestParseError::PestError(pest_err) => {
                DiagnosticInfo {
                    error_code: "P008".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Parser error: {}", pest_err.variant.message()),
                    primary_span: pest_error_location_to_source_span(pest_err), // MODIFIED
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new("Check the syntax of your input")],
                    notes: vec![format!("Details: {}", pest_err)],
                    caused_by: None,
                }
            }
        }
    }
}

// Helper function to convert pest::error::Error location to SourceSpan
pub fn pest_error_location_to_source_span(
    error: &pest::error::Error<super::Rule>,
) -> Option<SourceSpan> {
    match error.line_col {
        pest::error::LineColLocation::Pos((line, col)) => {
            let text = error.variant.message().to_string();
            Some(SourceSpan::new(line, col, line, col).with_source_text(text))
        }
        pest::error::LineColLocation::Span((start_line, start_col), (end_line, end_col)) => {
            let text = error.variant.message().to_string();
            Some(SourceSpan::new(start_line, start_col, end_line, end_col).with_source_text(text))
        }
    }
}
