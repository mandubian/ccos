use crate::ast::{Expression, TopLevel};
use crate::error_reporting::SourceSpan;
use crate::parser_error_reporter::{ParserError, ParserErrorReporter};
use pest::error::Error as PestError;
use pest::Parser;

// Declare submodules
pub mod common;
pub mod errors;
pub mod expressions;
pub mod special_forms;
pub mod toplevel;
pub mod types;
pub mod utils;

// Import items from submodules
pub use errors::PestParseError;
use expressions::build_expression;
use toplevel::build_ast;

// Define the parser struct using the grammar file
#[derive(pest_derive::Parser)]
#[grammar = "rtfs.pest"] // Path relative to src/
pub struct RTFSParser;

// Helper to create a SourceSpan from just the input text (for cases where we don't have a specific pair)
fn span_from_input(input: &str) -> Option<SourceSpan> {
    if input.is_empty() {
        return None;
    }

    let lines: Vec<&str> = input.lines().collect();
    let end_line = lines.len();
    let end_col = lines.last().map(|line| line.len()).unwrap_or(0);

    Some(SourceSpan::new(1, 1, end_line, end_col).with_source_text(input.to_string()))
}

// Helper function to build a program from pest pairs
fn build_program(pairs: pest::iterators::Pairs<Rule>) -> Result<Vec<TopLevel>, PestError<Rule>> {
    let program_content = pairs
        .peek()
        .expect("Parse should have yielded one program rule");
    let top_level_pairs = program_content.into_inner().filter(|p| {
        p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT && p.as_rule() != Rule::EOI
    });

    top_level_pairs
        .map(build_ast)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            PestError::new_from_pos(
                pest::error::ErrorVariant::CustomError {
                    message: format!("{:?}", e),
                },
                pest::Position::new("", 0).unwrap(),
            )
        })
}

// --- Main Parsing Functions ---

/// Parse a full RTFS program (potentially multiple top-level items)
pub fn parse(input: &str) -> Result<Vec<TopLevel>, PestError<Rule>> {
    let pairs = RTFSParser::parse(Rule::program, input)?;
    build_program(pairs)
}

/// Parse RTFS source code with enhanced error reporting
pub fn parse_with_enhanced_errors(
    source: &str,
    file_path: Option<&str>,
) -> Result<Vec<TopLevel>, ParserError> {
    match parse(source) {
        Ok(items) => Ok(items),
        Err(pest_error) => {
            let reporter = ParserErrorReporter::new();
            Err(reporter.report_error(pest_error, source, file_path))
        }
    }
}

/// Parse a single expression (useful for REPL or simple evaluation)

// ...

pub fn parse_expression(input: &str) -> Result<Expression, PestParseError> {
    let pairs = RTFSParser::parse(Rule::expression, input).map_err(PestParseError::from)?;
    let expr_pair = pairs.peek().ok_or_else(|| PestParseError::InvalidInput {
        message: "No expression found".to_string(),
        span: span_from_input(input),
    })?;
    let expression = build_expression(expr_pair)?;
    Ok(expression)
}

/// Parse a type expression (useful for type validation and capability schemas)
pub fn parse_type_expression(input: &str) -> Result<crate::ast::TypeExpr, PestParseError> {
    let pairs = RTFSParser::parse(Rule::type_expr, input).map_err(PestParseError::from)?;
    let type_pair = pairs.peek().ok_or_else(|| PestParseError::InvalidInput {
        message: "No type expression found".to_string(),
        span: span_from_input(input),
    })?;
    types::build_type_expr(type_pair)
}
