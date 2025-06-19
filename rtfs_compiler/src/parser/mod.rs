use pest::iterators::{Pair, Pairs}; // Added Pairs
use pest::Parser;
use crate::error_reporting::{SourceSpan, DiagnosticInfo, ErrorSeverity, ErrorHint};

// Helper function to convert pest span to our SourceSpan
pub fn pest_span_to_source_span(span: pest::Span) -> SourceSpan {
    let (start_line, start_column) = span.start_pos().line_col();
    let (end_line, end_column) = span.end_pos().line_col();
    SourceSpan::new(start_line, start_column, end_line, end_column)
        .with_source_text(span.as_str().to_string())
}

// Helper function to create SourceSpan from a Pair
pub fn pair_to_source_span(pair: &Pair<Rule>) -> SourceSpan {
    pest_span_to_source_span(pair.as_span())
}

// Helper functions to create errors with proper spans
pub fn missing_token_error(token: &str, pair: &Pair<Rule>) -> PestParseError {
    PestParseError::MissingToken {
        token: token.to_string(),
        span: Some(pair_to_source_span(pair)),
    }
}

pub fn invalid_input_error(message: &str, pair: &Pair<Rule>) -> PestParseError {
    PestParseError::InvalidInput {
        message: message.to_string(),
        span: Some(pair_to_source_span(pair)),
    }
}

pub fn invalid_literal_error(message: &str, pair: &Pair<Rule>) -> PestParseError {
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
        span: Option<SourceSpan>, // Add source location
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
    PestError(pest::error::Error<Rule>), // For errors from Pest itself
}

impl From<pest::error::Error<Rule>> for PestParseError {
    fn from(err: pest::error::Error<Rule>) -> Self {
        PestParseError::PestError(err)
    }
}

impl PestParseError {
    /// Convert the parse error to a diagnostic info for enhanced error reporting
    pub fn to_diagnostic(&self) -> DiagnosticInfo {
        match self {
            PestParseError::UnexpectedRule { expected, found, rule_text, span } => {
                DiagnosticInfo {
                    error_code: "P001".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Expected {}, found {}", expected, found),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new(&format!("Expected a {} expression here", expected))],
                    notes: vec![format!("Rule text: {}", rule_text)],
                    caused_by: None,
                }
            }
            PestParseError::MissingToken { token, span } => {
                DiagnosticInfo {
                    error_code: "P002".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Missing required token: {}", token),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new(&format!("Add the missing {} token", token))],
                    notes: vec![],
                    caused_by: None,
                }
            }
            PestParseError::InvalidInput { message, span } => {
                DiagnosticInfo {
                    error_code: "P003".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: message.clone(),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new("Check the syntax of your input")],
                    notes: vec![],
                    caused_by: None,
                }
            }
            PestParseError::UnsupportedRule { rule, span } => {
                DiagnosticInfo {
                    error_code: "P004".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Unsupported rule: {}", rule),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new(&format!("The {} construct is not yet supported", rule))],
                    notes: vec![],
                    caused_by: None,
                }
            }
            PestParseError::InvalidLiteral { message, span } => {
                DiagnosticInfo {
                    error_code: "P005".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: message.clone(),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new("Check the literal syntax")],
                    notes: vec![],
                    caused_by: None,
                }
            }
            PestParseError::InvalidEscapeSequence { sequence, span } => {
                DiagnosticInfo {
                    error_code: "P006".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: format!("Invalid escape sequence: {}", sequence),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![ErrorHint::new("Use valid escape sequences like \\n, \\t, \\r, \\\\, or \\\"")],
                    notes: vec![],
                    caused_by: None,
                }
            }
            PestParseError::CustomError { message, span } => {
                DiagnosticInfo {
                    error_code: "P007".to_string(),
                    severity: ErrorSeverity::Error,
                    primary_message: message.clone(),
                    primary_span: span.clone(),
                    secondary_spans: vec![],
                    hints: vec![],
                    notes: vec![],
                    caused_by: None,
                }
            }
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

// Re-export Rule from the generated parser
// pub use pest::RuleType; // Make RuleType usable - Commented out, seems unused directly

// Declare submodules
pub mod common;
pub mod expressions;
pub mod special_forms;
pub mod types;
pub mod utils;

// Import AST types needed at this level
use crate::ast::{
    Expression,
    ImportDefinition,
    ModuleDefinition,
    ModuleLevelDefinition,
    Symbol, // Ensure Symbol is imported
    TaskDefinition,
    TopLevel,
};

// Import builder functions from submodules
use common::build_symbol;
use expressions::{build_expression, build_map}; // Added build_map
use utils::unescape; // Added def/defn builders

// Define the parser struct using the grammar file
#[derive(pest_derive::Parser)]
#[grammar = "rtfs.pest"] // Path relative to src/
struct RTFSParser;

// Helper to skip whitespace and comments in a Pairs iterator
fn next_significant<'a>(pairs: &mut Pairs<'a, Rule>) -> Option<Pair<'a, Rule>> {
    pairs.find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
}

// Helper to convert Pest span to our SourceSpan format
fn span_from_pair(pair: &Pair<Rule>) -> Option<SourceSpan> {
    let span = pair.as_span();
    let (start_line, start_col) = span.start_pos().line_col();
    let (end_line, end_col) = span.end_pos().line_col();
    
    Some(SourceSpan::new(start_line, start_col, end_line, end_col)
        .with_source_text(span.as_str().to_string()))
}

// Helper to create a SourceSpan from just the input text (for cases where we don't have a specific pair)
fn span_from_input(input: &str) -> Option<SourceSpan> {
    if input.is_empty() {
        return None;
    }
    
    let lines: Vec<&str> = input.lines().collect();
    let end_line = lines.len();
    let end_col = lines.last().map(|line| line.len()).unwrap_or(0);
    
    Some(SourceSpan::new(1, 1, end_line, end_col)
        .with_source_text(input.to_string()))
}

// --- Main Parsing Function ---

// Parse a full RTFS program (potentially multiple top-level items)
pub fn parse(input: &str) -> Result<Vec<TopLevel>, PestParseError> {
    // MODIFIED error type
    let pairs = RTFSParser::parse(Rule::program, input).map_err(PestParseError::from)?; // MODIFIED to map error
                                                                                        // Program contains SOI ~ (task_definition | module_definition | expression)* ~ EOI
                                                                                        // The `pairs` variable is an iterator over the content matched by Rule::program.
                                                                                        // We need its single inner item (which should be the sequence inside program)
    let program_content = pairs
        .peek()
        .expect("Parse should have yielded one program rule");
    let top_level_pairs = program_content.into_inner().filter(|p| {
        p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT && p.as_rule() != Rule::EOI
    });
    // Keep EOI filter as it's implicitly added by Pest

    // Ok(top_level_pairs.map(build_ast).collect()) // OLD
    top_level_pairs
        .map(build_ast)
        .collect::<Result<Vec<_>, _>>() // NEW
}

// Parse a single expression (useful for REPL or simple evaluation)
pub fn parse_expression(input: &str) -> Result<Expression, PestParseError> {
    let pairs = RTFSParser::parse(Rule::expression, input).map_err(PestParseError::from)?;
    let expr_pair = pairs
        .peek()
        .ok_or_else(|| PestParseError::InvalidInput {
            message: "No expression found".to_string(),
            span: span_from_input(input),
        })?;
    build_expression(expr_pair)
}

// --- AST Builder Functions ---

// fn build_ast(pair: Pair<Rule>) -> TopLevel {
fn build_ast(pair: Pair<Rule>) -> Result<TopLevel, PestParseError> {
    // MODIFIED return type
    match pair.as_rule() {
        // Handle expression directly or via special form rules that resolve to Expression
        Rule::expression
        | Rule::literal
        | Rule::symbol
        | Rule::keyword
        | Rule::list
        | Rule::vector
        | Rule::map
        | Rule::let_expr
        | Rule::if_expr
        | Rule::do_expr
        | Rule::fn_expr
        | Rule::def_expr // def/defn can appear outside modules (though maybe discouraged)
        | Rule::defn_expr
        | Rule::parallel_expr
        | Rule::with_resource_expr
        | Rule::try_catch_expr
        | Rule::match_expr
        | Rule::log_step_expr
        | Rule::identifier // Allow standalone identifiers? Maybe error later.
        // | Rule::namespaced_identifier => Ok(TopLevel::Expression(build_expression(pair?))), // MODIFIED OLD
        | Rule::namespaced_identifier => build_expression(pair).map(TopLevel::Expression), // MODIFIED NEW

        // Handle specific top-level definitions
        // Rule::task_definition => Ok(TopLevel::Task(build_task_definition(pair?)), // MODIFIED OLD
        Rule::task_definition => build_task_definition(pair).map(TopLevel::Task), // MODIFIED NEW
        // Rule::module_definition => Ok(TopLevel::Module(build_module_definition(pair?))), // MODIFIED OLD
        Rule::module_definition => build_module_definition(pair).map(TopLevel::Module), // MODIFIED NEW        // Import definition should only appear inside a module, handle within build_module_definition
        Rule::import_definition => {
            // panic!("Import definition found outside of a module context") // OLD
            Err(PestParseError::CustomError { // NEW
                message: "Import definition found outside of a module context".to_string(),
                span: span_from_pair(&pair),
            })
        }

        // Handle unexpected rules at this level
        // rule => unimplemented!( // OLD
        //     "build_ast encountered unexpected top-level rule: {:?}, content: \'{}\'",
        //     rule,
        //     pair.as_str()
        // ),
        rule => Err(PestParseError::CustomError { // NEW
            message: format!(
                "build_ast encountered unexpected top-level rule: {:?}, content: '{}'",
                rule,
                pair.as_str()
            ),
            span: span_from_pair(&pair),
        }),
    }
}

// --- Top-Level Builders ---

// task_definition =  { "(" ~ "task" ~ task_property+ ~ ")" }
fn build_task_definition(pair: Pair<Rule>) -> Result<TaskDefinition, PestParseError> {
    let mut inner = pair.into_inner(); // Skip '(' and 'task'

    let mut id = None;
    let mut source = None;
    let mut timestamp = None;
    let mut metadata = None;
    let mut intent = None;
    let mut contracts = None;
    let mut plan = None;
    let mut execution_trace = None;

    // Skip the 'task' keyword
    let _task_keyword = next_significant(&mut inner);

    while let Some(prop_pair) = next_significant(&mut inner) {
        eprintln!(
            "[build_task_definition] prop_pair: rule={:?}, str='{}'",
            prop_pair.as_rule(),
            prop_pair.as_str()
        );
        let prop_str = prop_pair.as_str().to_string();
        let mut prop_inner = prop_pair.into_inner();
        let value_pair = next_significant(&mut prop_inner).expect("Task property needs value");
        eprintln!(
            "[build_task_definition] value_pair: rule={:?}, str='{}'",
            value_pair.as_rule(),
            value_pair.as_str()
        );
        match prop_str.trim_start() {
            s if s.starts_with(":id") => {
                assert_eq!(value_pair.as_rule(), Rule::string);
                let raw_str = value_pair.as_str();
                let content = &raw_str[1..raw_str.len() - 1];
                id = Some(unescape(content).expect("Invalid escape sequence in task id"));
            }
            s if s.starts_with(":source") => {
                assert_eq!(value_pair.as_rule(), Rule::string);
                let raw_str = value_pair.as_str();
                let content = &raw_str[1..raw_str.len() - 1];
                source = Some(unescape(content).expect("Invalid escape sequence in task source"));
            }
            s if s.starts_with(":timestamp") => {
                assert_eq!(value_pair.as_rule(), Rule::string);
                let raw_str = value_pair.as_str();
                let content = &raw_str[1..raw_str.len() - 1];
                timestamp =
                    Some(unescape(content).expect("Invalid escape sequence in task timestamp"));
            }
            s if s.starts_with(":metadata") => {
                assert_eq!(value_pair.as_rule(), Rule::map);
                metadata = Some(Expression::Map(build_map(value_pair)?));
            }
            s if s.starts_with(":intent") => {
                intent = Some(build_expression(value_pair)?);
            }
            s if s.starts_with(":contracts") => {
                assert_eq!(value_pair.as_rule(), Rule::map);
                contracts = Some(Expression::Map(build_map(value_pair)?));
            }
            s if s.starts_with(":plan") => {
                plan = Some(build_expression(value_pair)?);
            }
            s if s.starts_with(":execution-trace") => {
                assert_eq!(value_pair.as_rule(), Rule::vector);
                // execution_trace = Some(Expression::Vector( // OLD
                //     value_pair.into_inner().map(build_expression).collect(),
                // ));
                let exprs = value_pair // NEW
                    .into_inner()
                    .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                    .map(build_expression)
                    .collect::<Result<Vec<_>, PestParseError>>()?;
                execution_trace = Some(Expression::Vector(exprs)); // NEW
            }
            _ => panic!("Unknown task property: {}", prop_str),
        }
    }

    Ok(TaskDefinition {
        id,
        source,
        timestamp,
        intent,
        contracts,
        plan,
        execution_trace,
        metadata,
    })
}

// Helper function to convert pest::error::Error location to SourceSpan
fn pest_error_location_to_source_span(error: &pest::error::Error<Rule>) -> Option<SourceSpan> {
    match error.line_col {
        pest::error::LineColLocation::Pos((line, col)) => {
            // For a single position, we might not have the full text of the error span directly from pest's error struct easily,
            // so we use the error message itself as a placeholder or rely on the line/col.
            // The original text snippet that caused the error is part of pest::Error::variant (e.g., for UnexpectedToken).
            // However, constructing a meaningful SourceSpan text from just Pos can be tricky.
            // Let's try to get the text from the error variant if possible, or default to a generic message.
            let text = error.variant.message().to_string(); // Or a snippet if accessible
            Some(SourceSpan::new(line, col, line, col).with_source_text(text))
        }
        pest::error::LineColLocation::Span((start_line, start_col), (end_line, end_col)) => {
            // If we have a span, we can try to get the text.
            // The `error.variant.message()` gives the description, not necessarily the spanned text.
            // For now, using the message as text. A more advanced way would be to re-read from source if available.
            let text = error.variant.message().to_string();
            Some(SourceSpan::new(start_line, start_col, end_line, end_col).with_source_text(text))
        }
    }
}

// Helper function to build export options
// MODIFIED: Added parent_pair for better error spanning
fn build_export_option(parent_pair: &Pair<Rule>, mut pairs: Pairs<Rule>) -> Result<Vec<Symbol>, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair); // Use pair_to_source_span
    let exports_keyword_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Expected :exports keyword in export_option".to_string(),
            span: Some(parent_span.clone()), // MODIFIED
        }
    })?;
    if exports_keyword_pair.as_rule() != Rule::exports_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: ":exports keyword".to_string(),
            found: format!("{:?}", exports_keyword_pair.as_rule()),
            rule_text: exports_keyword_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair)), // Use pair_to_source_span
        });
    }

    let symbols_vec_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Expected symbols vector in export_option".to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair).end_as_start()), // MODIFIED: Span after the keyword
        }
    })?;
    if symbols_vec_pair.as_rule() != Rule::export_symbols_vec {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbols vector (export_symbols_vec)".to_string(),
            found: format!("{:?}", symbols_vec_pair.as_rule()),
            rule_text: symbols_vec_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&symbols_vec_pair)), 
        });
    }

    symbols_vec_pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::symbol)
        .map(|p| build_symbol(p.clone())) // build_symbol now returns Result, ensure pair is cloned if needed by build_symbol for span
        .collect::<Result<Vec<Symbol>, PestParseError>>()
}

// module_definition =  { "(" ~ module_keyword ~ symbol ~ export_option? ~ definition* ~ ")" }
// fn build_module_definition(pair: Pair<Rule>) -> ModuleDefinition { // Old signature
fn build_module_definition(pair: Pair<Rule>) -> Result<ModuleDefinition, PestParseError> {
    // New signature
    let module_def_span = pair_to_source_span(&pair); // Use pair_to_source_span
    let mut inner_pairs = pair.clone().into_inner(); 
    // 1. module_keyword
    let module_keyword_pair = next_significant(&mut inner_pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Module definition missing module keyword".to_string(),
            span: Some(module_def_span.clone()), 
        }
    })?;
    if module_keyword_pair.as_rule() != Rule::module_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "module_keyword".to_string(),
            found: format!("{:?}", module_keyword_pair.as_rule()),
            rule_text: module_keyword_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&module_keyword_pair)),
        });
    }

    // 2. Name (symbol)
    let name_pair = next_significant(&mut inner_pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Module definition requires a name".to_string(),
            span: Some(pair_to_source_span(&module_keyword_pair).end_as_start()), 
        }
    })?;
    if name_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol for module name".to_string(),
            found: format!("{:?}", name_pair.as_rule()),
            rule_text: name_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&name_pair)), 
        });
    }
    let name = build_symbol(name_pair.clone())?;

    let mut exports = None;
    let mut definitions = Vec::new();

    let mut remaining_module_parts = inner_pairs.peekable();

    if let Some(peeked_part) = remaining_module_parts.peek() {
        if peeked_part.as_rule() == Rule::export_option {
            let export_pair = remaining_module_parts.next().unwrap(); 
            exports = Some(build_export_option(&export_pair, export_pair.clone().into_inner())?); 
        }
    }
    
    for def_candidate_pair in remaining_module_parts {
        match def_candidate_pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT | Rule::EOI => continue, 
            Rule::def_expr => {
                let def_node = special_forms::build_def_expr(def_candidate_pair)?;
                definitions.push(ModuleLevelDefinition::Def(def_node));
            }
            Rule::defn_expr => {
                let defn_node = special_forms::build_defn_expr(def_candidate_pair)?;
                definitions.push(ModuleLevelDefinition::Defn(defn_node));
            }
            Rule::import_definition => {
                let import_node = build_import_definition(&def_candidate_pair, def_candidate_pair.clone().into_inner())?;
                definitions.push(ModuleLevelDefinition::Import(import_node));
            }            
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "def_expr, defn_expr, or import_definition".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: def_candidate_pair.as_str().to_string(),
                    span: Some(pair_to_source_span(&def_candidate_pair)),
                });
            }
        }
    }

    Ok(ModuleDefinition {
        name,
        exports,
        definitions,
    })
}

// import_definition =  { "(" ~ import_keyword ~ namespaced_identifier ~ import_options? ~ ")" }
// fn build_import_definition(pair: Pair<Rule>) -> ImportDefinition { // Old signature
fn build_import_definition(parent_pair: &Pair<Rule>, mut pairs: Pairs<Rule>) -> Result<ImportDefinition, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let import_keyword_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Import definition missing import keyword".to_string(),
            span: Some(parent_span.clone()), 
        }
    })?;
    if import_keyword_pair.as_rule() != Rule::import_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "import_keyword".to_string(),
            found: format!("{:?}", import_keyword_pair.as_rule()),
            rule_text: import_keyword_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&import_keyword_pair)),
        });
    }

    let module_name_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError {
            message: "Import definition requires a module name".to_string(),
            span: Some(pair_to_source_span(&import_keyword_pair).end_as_start()), 
        }
    })?;
    if !(module_name_pair.as_rule() == Rule::namespaced_identifier
        || module_name_pair.as_rule() == Rule::symbol)
    {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol or namespaced_identifier for import module name".to_string(),
            found: format!("{:?}", module_name_pair.as_rule()),
            rule_text: module_name_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&module_name_pair)),
        });
    }
    let module_name = build_symbol(module_name_pair.clone())?;    let mut alias = None;
    let mut only = None;

    while let Some(option_pair) = next_significant(&mut pairs) { 
        let current_option_span = pair_to_source_span(&option_pair);
        
        if option_pair.as_rule() == Rule::import_option {
            let option_text = option_pair.as_str();
            
            // Check which branch of the import_option rule was matched
            if option_text.starts_with(":as") {
                // ":as" ~ symbol branch
                let mut option_inner_pairs = option_pair.clone().into_inner();
                let alias_symbol_pair = option_inner_pairs.next().ok_or_else(|| PestParseError::CustomError {
                    message: "Import :as option missing symbol".to_string(),
                    span: Some(current_option_span.end_as_start()), 
                })?;
                if alias_symbol_pair.as_rule() == Rule::symbol {
                    alias = Some(build_symbol(alias_symbol_pair.clone())?);
                } else {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol for :as alias".to_string(),
                        found: format!("{:?}", alias_symbol_pair.as_rule()),
                        rule_text: alias_symbol_pair.as_str().to_string(),
                        span: Some(pair_to_source_span(&alias_symbol_pair)),
                    });
                }
            } else if option_text.starts_with(":only") {
                // ":only" ~ "[" ~ symbol+ ~ "]" branch
                let collected_symbols: Result<Vec<Symbol>, PestParseError> =
                    option_pair.clone().into_inner()
                        .filter(|p| p.as_rule() == Rule::symbol) 
                        .map(|p| build_symbol(p.clone())) 
                        .collect();                
                let symbols = collected_symbols?; 
                
                if symbols.is_empty() {
                    return Err(PestParseError::CustomError {
                        message: "Import :only option requires at least one symbol".to_string(),
                        span: Some(current_option_span.end_as_start()), 
                    });
                }
                only = Some(symbols); 
            } else {
                return Err(PestParseError::CustomError { 
                    message: format!("Unknown import option structure. Expected ':as' or ':only' prefix, found '{}'", option_text),
                    span: Some(current_option_span), 
                });
            }
        } else {
            // Handle cases where import options might not be wrapped in import_option rule
            // This could happen if pest is flattening the structure
            match option_pair.as_str() {
                ":as" => {
                    // Expect the next token to be a symbol
                    let alias_symbol_pair = next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
                        message: "Import :as option missing symbol".to_string(),
                        span: Some(current_option_span.end_as_start()), 
                    })?;
                    if alias_symbol_pair.as_rule() == Rule::symbol {
                        alias = Some(build_symbol(alias_symbol_pair.clone())?);
                    } else {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "symbol for :as alias".to_string(),
                            found: format!("{:?}", alias_symbol_pair.as_rule()),
                            rule_text: alias_symbol_pair.as_str().to_string(),
                            span: Some(pair_to_source_span(&alias_symbol_pair)),
                        });
                    }
                }
                ":only" => {
                    // Expect the next token to be the opening bracket or symbols
                    let mut only_symbols = Vec::new();
                    let mut found_opening_bracket = false;
                    
                    // Look for symbols until we find the closing bracket or end of input
                    while let Some(next_pair) = pairs.peek() {
                        if next_pair.as_str() == "[" {
                            found_opening_bracket = true;
                            pairs.next(); // consume the opening bracket
                            continue;
                        } else if next_pair.as_str() == "]" {
                            pairs.next(); // consume the closing bracket
                            break;
                        } else if next_pair.as_rule() == Rule::symbol {
                            let symbol_pair = pairs.next().unwrap();
                            only_symbols.push(build_symbol(symbol_pair)?);
                        } else {
                            break;
                        }
                    }
                    
                    if only_symbols.is_empty() {
                        return Err(PestParseError::CustomError {
                            message: "Import :only option requires at least one symbol".to_string(),
                            span: Some(current_option_span.end_as_start()), 
                        });
                    }
                    only = Some(only_symbols);
                }
                _ => {
                    return Err(PestParseError::UnexpectedRule { 
                        expected: "import_option (:as or :only) or end of statement".to_string(), 
                        found: format!("rule: {:?}", option_pair.as_rule()),
                        rule_text: option_pair.as_str().to_string(),
                        span: Some(current_option_span), 
                    });
                }
            }
        }
    }
    Ok(ImportDefinition {
        module_name,
        alias,
        only,
    })
}

// Optional: Add tests within this module or a separate tests submodule
#[cfg(test)]
mod tests {
    use super::*;
    // Move AST imports needed only for tests here
    use crate::ast::{
        CatchClause,
        CatchPattern,
        DefExpr,
        DefnExpr,
        DoExpr,
        Expression,
        ImportDefinition,
        Keyword,
        LetBinding,        LetExpr,
        Literal,        MapKey,
        MatchClause,
        MatchExpr,
        MatchPattern,
        ModuleDefinition,
        ModuleLevelDefinition,
        ParallelBinding,
        ParallelExpr,
        ParamDef,
        Pattern,
        Symbol,
        TaskDefinition,
        TopLevel,
        TryCatchExpr,
        TypeExpr,
        WithResourceExpr,
    };
    // use crate::parser::types::build_type_expr; // Removed unused import
    use std::collections::HashMap;

    // Helper macro for asserting expression parsing
    macro_rules! assert_expr_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = RTFSParser::parse(Rule::expression, $input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse expression (RTFSParser::parse):\\\\nInput: {:?}\\\\nError: {:?}",
                $input,
                parse_result.err().unwrap()
            );
            let expr_pair = parse_result.unwrap().next().unwrap();
            let expr_pair_str = expr_pair.as_str().to_string();
            let ast_result = expressions::build_expression(expr_pair);
            assert!(
                ast_result.is_ok(),
                "Failed to build expression (expressions::build_expression):\\\\nInput: {:?}\\\\nSource pair: {:?}\\\\nError: {:?}",
                $input,
                expr_pair_str,
                ast_result.err().unwrap()
            );
            let ast = ast_result.unwrap();            if ast != $expected {
                println!("Expression AST mismatch for input: {:?}", $input);
                println!("Expected: {:#?}", $expected);
                println!("Actual: {:#?}", ast);
                panic!("AST mismatch");
            }
        };
    }

    // Helper macro for asserting top-level parsing
    macro_rules! assert_program_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = parse($input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse program:\\\\nInput: {:?}\\\\nError: {:?}",
                $input,
                parse_result.err().unwrap()
            );
            let ast_vec = parse_result.unwrap();
            assert_eq!(
                ast_vec, $expected,
                "Program AST mismatch for input: {:?}",
                $input
            );
        };
    }

    #[test]
    fn test_parse_simple_literals() {
        assert_expr_parses_to!("123", Expression::Literal(Literal::Integer(123)));
        assert_expr_parses_to!("-45", Expression::Literal(Literal::Integer(-45)));
        assert_expr_parses_to!("1.23", Expression::Literal(Literal::Float(1.23)));
        assert_expr_parses_to!("-0.5", Expression::Literal(Literal::Float(-0.5)));
        assert_expr_parses_to!(
            r#""hello""#,
            Expression::Literal(Literal::String("hello".to_string()))
        );
        assert_expr_parses_to!(
            r#""hello\\world\n""#,
            Expression::Literal(Literal::String("hello\\world\n".to_string()))
        );
        assert_expr_parses_to!("true", Expression::Literal(Literal::Boolean(true)));
        assert_expr_parses_to!("false", Expression::Literal(Literal::Boolean(false)));
        assert_expr_parses_to!("nil", Expression::Literal(Literal::Nil));
    }

    #[test]
    fn test_parse_symbol_keyword() {
        assert_expr_parses_to!(
            "my-var",
            Expression::Symbol(Symbol("my-var".to_string()))
        );
        assert_expr_parses_to!(
            "ns/my-var",
            Expression::Symbol(Symbol("ns/my-var".to_string()))
        );
        assert_expr_parses_to!(
            ":my-key",
            Expression::Literal(Literal::Keyword(Keyword("my-key".to_string())))
        );
    }

    #[test]
    fn test_parse_collections() {
        // Vector
        assert_expr_parses_to!(
            r#"[1 2 "three"]"#,
            Expression::Vector(vec![
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
                Expression::Literal(Literal::String("three".to_string())),
            ])
        );
        assert_expr_parses_to!("[]", Expression::Vector(vec![]));

        // List (Function Call heuristic)
        assert_expr_parses_to!(
            "(a b c)",
            Expression::FunctionCall {
                callee: Box::new(Expression::Symbol(Symbol("a".to_string()))),
                arguments: vec![
                    Expression::Symbol(Symbol("b".to_string())),
                    Expression::Symbol(Symbol("c".to_string())),
                ]
            }
        );
        // Empty list is still a list
        assert_expr_parses_to!("()", Expression::List(vec![]));
        // List starting with non-symbol is a list
        assert_expr_parses_to!(
            "(1 2 3)",
            Expression::List(vec![
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
                Expression::Literal(Literal::Integer(3)),
            ])
        );

        // Map
        let mut expected_map = HashMap::new();
        expected_map.insert(
            MapKey::Keyword(Keyword("a".to_string())),
            Expression::Literal(Literal::Integer(1)),
        );
        expected_map.insert(
            MapKey::String("b".to_string()),
            Expression::Literal(Literal::Boolean(true)),
        );
        assert_expr_parses_to!(
            r#"{ :a 1 "b" true }"#,
            Expression::Map(expected_map.clone())
        );
        assert_expr_parses_to!("{}", Expression::Map(HashMap::new()));

        // Map with integer key
        let mut map_with_int_key = HashMap::new();
        map_with_int_key.insert(
            MapKey::Integer(0),
            Expression::Literal(Literal::String("zero".to_string())),
        );
        map_with_int_key.insert(
            MapKey::Keyword(Keyword("a".to_string())),
            Expression::Literal(Literal::Integer(1)),
        );
        assert_expr_parses_to!(
            r#"{0 "zero" :a 1}"#,
            Expression::Map(map_with_int_key.clone())
        );
    }

    #[test]
    fn test_parse_def() {
        assert_expr_parses_to!(
            "(def x 1)",
            Expression::Def(Box::new(DefExpr {
                symbol: Symbol("x".to_string()),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(1))),
            }))
        );
        assert_expr_parses_to!(
            r#"(def y :MyType "value")"#,
            Expression::Def(Box::new(DefExpr {
                symbol: Symbol("y".to_string()),
                type_annotation: Some(TypeExpr::Alias(Symbol("MyType".to_string()))),
                value: Box::new(Expression::Literal(Literal::String("value".to_string()))),
            }))
        );
    }

    #[test]
    fn test_parse_let() {
        // Simple let
        assert_expr_parses_to!(
            r#"(let [x 1 y "hi"] (+ x 1))"#,
            Expression::Let(LetExpr {
                bindings: vec![
                    LetBinding {
                        pattern: Pattern::Symbol(Symbol("x".to_string())),
                        type_annotation: None,
                        value: Box::new(Expression::Literal(Literal::Integer(1))),
                    },
                    LetBinding {
                        pattern: Pattern::Symbol(Symbol("y".to_string())),
                        type_annotation: None,
                        value: Box::new(Expression::Literal(Literal::String("hi".to_string())))
                    },
                ],
                body: vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("x".to_string())),
                        Expression::Literal(Literal::Integer(1)),
                    ],
                },],
            })
        );
        // Let with vector destructuring
        assert_expr_parses_to!(
            "(let [[a b & rest :as all-v] my-vec x 1] (do a b rest all-v x))",
            Expression::Let(LetExpr {
                bindings: vec![
                    LetBinding {
                        pattern: Pattern::VectorDestructuring {
                            elements: vec![
                                Pattern::Symbol(Symbol("a".to_string())),
                                Pattern::Symbol(Symbol("b".to_string())),
                            ],
                            rest: Some(Symbol("rest".to_string())),
                            as_symbol: Some(Symbol("all-v".to_string())),
                        },
                        type_annotation: None,
                        value: Box::new(Expression::Symbol(Symbol("my-vec".to_string()))),
                    },
                    LetBinding {
                        pattern: Pattern::Symbol(Symbol("x".to_string())),
                        type_annotation: None,
                        value: Box::new(Expression::Literal(Literal::Integer(1))),
                    },
                ],
                body: vec![Expression::Do(DoExpr {
                    expressions: vec![
                        Expression::Symbol(Symbol("a".to_string())),
                        Expression::Symbol(Symbol("b".to_string())),
                        Expression::Symbol(Symbol("rest".to_string())),
                        Expression::Symbol(Symbol("all-v".to_string())),
                        Expression::Symbol(Symbol("x".to_string())),
                    ],
                })],
            })
        );
        // Let with map destructuring
        assert_expr_parses_to!(
            r#"(let [{:key1 val1 :keys [s1 s2] "str-key" val2 & r :as all-m} my-map] (do val1 s1 s2 val2 r all-m))"#,
            Expression::Let(LetExpr {
                bindings: vec![LetBinding {
                    pattern: Pattern::MapDestructuring {
                        entries: vec![
                            crate::ast::MapDestructuringEntry::KeyBinding {
                                key: MapKey::Keyword(Keyword("key1".to_string())),
                                pattern: Box::new(Pattern::Symbol(Symbol("val1".to_string()))),
                            },
                            crate::ast::MapDestructuringEntry::Keys(vec![
                                Symbol("s1".to_string()),
                                Symbol("s2".to_string()),
                            ]),
                            crate::ast::MapDestructuringEntry::KeyBinding {
                                key: MapKey::String("str-key".to_string()),
                                pattern: Box::new(Pattern::Symbol(Symbol("val2".to_string()))),
                            },
                        ],
                        rest: Some(Symbol("r".to_string())),
                        as_symbol: Some(Symbol("all-m".to_string())),
                    },
                    type_annotation: None,
                    value: Box::new(Expression::Symbol(Symbol("my-map".to_string()))),
                }],
                body: vec![Expression::Do(DoExpr {
                    expressions: vec![
                        Expression::Symbol(Symbol("val1".to_string())),
                        Expression::Symbol(Symbol("s1".to_string())),
                        Expression::Symbol(Symbol("s2".to_string())),
                        Expression::Symbol(Symbol("val2".to_string())),
                        Expression::Symbol(Symbol("r".to_string())),
                        Expression::Symbol(Symbol("all-m".to_string())),
                    ],
                })],
            })
        );
        // Let with wildcard
        assert_expr_parses_to!(
            "(let [_ 1 y 2] y)",
            Expression::Let(LetExpr {
                bindings: vec![
                    LetBinding {
                        pattern: Pattern::Wildcard,
                        type_annotation: None,
                        value: Box::new(Expression::Literal(Literal::Integer(1))),
                    },
                    LetBinding {
                        pattern: Pattern::Symbol(Symbol("y".to_string())),
                        type_annotation: None,
                        value: Box::new(Expression::Literal(Literal::Integer(2))),
                    },
                ],
                body: vec![Expression::Symbol(Symbol("y".to_string()))],
            })
        );
    }

    // --- Top Level Parsing ---
    #[test]
    fn test_parse_program_simple() {
        assert_program_parses_to!(
            "123",
            vec![TopLevel::Expression(Expression::Literal(Literal::Integer(
                123
            )))]
        );
        assert_program_parses_to!(
            r#"(def x 1)
; comment
"hello""#,
            vec![
                TopLevel::Expression(Expression::Def(Box::new(DefExpr {
                    symbol: Symbol("x".to_string()),
                    type_annotation: None,
                    value: Box::new(Expression::Literal(Literal::Integer(1))),
                }))),
                TopLevel::Expression(Expression::Literal(Literal::String("hello".to_string()))),
            ]
        );
        assert_program_parses_to!("", vec![]); // Empty program
    }

    #[test]
    fn test_parse_task_definition() {
        let input = r#"
        (task
          :id "task-123"
          :source "user-prompt"
          :intent (generate-code "Create a button")
          :contracts { :input :string :output :component }
          :plan (step-1 (step-2))
          :execution-trace [ { :step "step-1" :status :success } ]
        )
        "#;
        let mut contracts_map = HashMap::new();
        contracts_map.insert(
            MapKey::Keyword(Keyword("input".to_string())),
            Expression::Literal(Literal::Keyword(Keyword("string".to_string()))),
        );
        contracts_map.insert(
            MapKey::Keyword(Keyword("output".to_string())),
            Expression::Literal(Literal::Keyword(Keyword("component".to_string()))),
        );
        let mut trace_map = HashMap::new();
        trace_map.insert(
            MapKey::Keyword(Keyword("step".to_string())),
            Expression::Literal(Literal::String("step-1".to_string())),
        );
        trace_map.insert(
            MapKey::Keyword(Keyword("status".to_string())),
            Expression::Literal(Literal::Keyword(Keyword("success".to_string()))),
        );

        assert_program_parses_to!(
            input,
            vec![TopLevel::Task(TaskDefinition {
                id: Some("task-123".to_string()),
                source: Some("user-prompt".to_string()),
                timestamp: None,
                intent: Some(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("generate-code".to_string()))),
                    arguments: vec![Expression::Literal(Literal::String(
                        "Create a button".to_string()
                    ))],
                }),
                contracts: Some(Expression::Map(contracts_map)),
                plan: Some(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("step-1".to_string()))),
                    arguments: vec![Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("step-2".to_string()))),
                        arguments: vec![],
                    }],
                }),
                execution_trace: Some(Expression::Vector(vec![Expression::Map(trace_map)])),
                metadata: None,
            })]
        );
    }

    #[test]
    fn test_parse_module_definition() {
        let input = r#"
        (module my.cool-module
          (:exports [ public-fn ])
          (import other.module :as other)
          (import another.module :only [ func-a func-b ])
          (def private-val 42)
          (defn public-fn [x :ParamType & rest-args :RestType] :ReturnType
            (other/do-something x private-val rest-args))
        )
        "#;
        let expected = vec![TopLevel::Module(ModuleDefinition {
            name: Symbol("my.cool-module".to_string()),
            exports: Some(vec![Symbol("public-fn".to_string())]),
            definitions: vec![
                ModuleLevelDefinition::Import(ImportDefinition {
                    module_name: Symbol("other.module".to_string()),
                    alias: Some(Symbol("other".to_string())),
                    only: None,
                }),
                ModuleLevelDefinition::Import(ImportDefinition {
                    module_name: Symbol("another.module".to_string()),
                    alias: None,
                    only: Some(vec![
                        Symbol("func-a".to_string()),
                        Symbol("func-b".to_string()),
                    ]),
                }),
                ModuleLevelDefinition::Def(DefExpr {
                    symbol: Symbol("private-val".to_string()),
                    type_annotation: None,
                    value: Box::new(Expression::Literal(Literal::Integer(42))),
                }),
                ModuleLevelDefinition::Defn(DefnExpr {
                    name: Symbol("public-fn".to_string()),
                    params: vec![ParamDef {
                        pattern: Pattern::Symbol(Symbol("x".to_string())),
                        type_annotation: Some(TypeExpr::Alias(Symbol("ParamType".to_string()))),
                    }],
                    variadic_param: Some(ParamDef {
                        pattern: Pattern::Symbol(Symbol("rest-args".to_string())),
                        type_annotation: Some(TypeExpr::Alias(Symbol("RestType".to_string()))),
                    }),
                    return_type: Some(TypeExpr::Alias(Symbol("ReturnType".to_string()))),
                    body: vec![Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("other/do-something".to_string()))),
                        arguments: vec![
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Symbol(Symbol("private-val".to_string())),
                            Expression::Symbol(Symbol("rest-args".to_string())),
                        ],
                    }],
                }),
            ],
        })];
        assert_program_parses_to!(input, expected);
    }

    // --- Tests for New Special Forms ---

    #[test]
    fn test_parse_parallel() {
        assert_expr_parses_to!(
            "(parallel [a (f 1)] [b :SomeType (g 2)])",
            Expression::Parallel(ParallelExpr {
                bindings: vec![
                    ParallelBinding {
                        symbol: Symbol("a".to_string()),
                        type_annotation: None,
                        expression: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("f".to_string()))),
                            arguments: vec![Expression::Literal(Literal::Integer(1))],
                        }),
                    },
                    ParallelBinding {
                        symbol: Symbol("b".to_string()),
                        type_annotation: Some(TypeExpr::Alias(Symbol("SomeType".to_string()))),
                        expression: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("g".to_string()))),
                            arguments: vec![Expression::Literal(Literal::Integer(2))],
                        }),
                    },
                ]
            })
        );
    }

    #[test]
    fn test_parse_with_resource() {
        assert_expr_parses_to!(
            "(with-resource [res ResourceType (init-res)] (use res))",
            Expression::WithResource(WithResourceExpr {
                resource_symbol: Symbol("res".to_string()),
                resource_type: TypeExpr::Alias(Symbol("ResourceType".to_string())),
                resource_init: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("init-res".to_string()))),
                    arguments: vec![],
                }),
                body: vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("use".to_string()))),
                    arguments: vec![Expression::Symbol(Symbol("res".to_string()))],
                }],
            })
        );
    }

    #[test]
    fn test_parse_try_catch() {
        // Basic try-catch
        assert_expr_parses_to!(
            "(try (dangerous-op) (catch :Error e (log e)) (catch :OtherError oe (log oe)))",
            Expression::TryCatch(TryCatchExpr {
                try_body: vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("dangerous-op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("Error".to_string())),
                        binding: Symbol("e".to_string()),
                        body: vec![Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("e".to_string()))],
                        }],
                    },
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("OtherError".to_string())),
                        binding: Symbol("oe".to_string()),
                        body: vec![Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("oe".to_string()))],
                        }],
                    },
                ],
                finally_body: None,
            })
        );

        // Try-catch with finally
        assert_expr_parses_to!(
            "(try (op) (catch :E e (log e)) (finally (cleanup)))",
            Expression::TryCatch(TryCatchExpr {
                try_body: vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![CatchClause {
                    pattern: CatchPattern::Keyword(Keyword("E".to_string())),
                    binding: Symbol("e".to_string()),
                    body: vec![Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                        arguments: vec![Expression::Symbol(Symbol("e".to_string()))],
                    }],
                }],
                finally_body: Some(vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("cleanup".to_string()))),
                    arguments: vec![],
                }]),
            })
        );

        // Try-finally (no catch)
        assert_expr_parses_to!(
            "(try (main-op) (finally (always-run)))",
            Expression::TryCatch(TryCatchExpr {
                try_body: vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("main-op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![],
                finally_body: Some(vec![Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("always-run".to_string()))),
                    arguments: vec![],
                }]),
            })
        );
    }    #[test]
    fn test_parse_match() {
        // Basic match expression (this should work)        
        assert_expr_parses_to!(
            r#"(match my-val 1 "one" [2 3] "two-three" _ "default")"#,
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("my-val".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Literal(Literal::Integer(1)),
                        guard: None,
                        body: Box::new(Expression::Literal(Literal::String("one".to_string()))),
                    },
                    MatchClause {
                        pattern: MatchPattern::Vector {
                            elements: vec![
                                MatchPattern::Literal(Literal::Integer(2)),
                                MatchPattern::Literal(Literal::Integer(3)),
                            ],
                            rest: None,
                        },
                        guard: None,
                        body: Box::new(Expression::Literal(Literal::String(
                            "two-three".to_string()
                        ))),
                    },
                    MatchClause {                        
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: Box::new(Expression::Literal(Literal::String("default".to_string()))),
                    },
                ],
            })
        );
    }

    #[test]
    fn test_parse_match_with_guard() {
        // Test guard functionality with 'when' keyword
        assert_expr_parses_to!(
            "(match x [a b] when (> a b) (combine a b) _ nil)",
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("x".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Vector {
                            elements: vec![
                                MatchPattern::Symbol(Symbol("a".to_string())),
                                MatchPattern::Symbol(Symbol("b".to_string())),
                            ],
                            rest: None,
                        },
                        guard: Some(Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol(">".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("a".to_string())),
                                Expression::Symbol(Symbol("b".to_string())),
                            ],
                        })),
                        body: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("combine".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("a".to_string())),
                                Expression::Symbol(Symbol("b".to_string())),
                            ],
                        }),
                    },                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: Box::new(Expression::Literal(Literal::Nil)),
                    },
                ],
            })
        );
    }    #[test]
    fn test_parse_match_with_map() {
        // Test map pattern matching functionality
        assert_expr_parses_to!(            r#"(match data {:type "user" :name n} (greet n) { :type "admin" } (admin-panel))"#,
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("data".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Map {
                            entries: vec![
                                crate::ast::MapMatchEntry {
                                    key: MapKey::Keyword(Keyword("type".to_string())),
                                    pattern: Box::new(MatchPattern::Literal(Literal::String(
                                        "user".to_string()
                                    ))),
                                },
                                crate::ast::MapMatchEntry {
                                    key: MapKey::Keyword(Keyword("name".to_string())),
                                    pattern: Box::new(MatchPattern::Symbol(Symbol(
                                        "n".to_string()
                                    ))),
                                },
                            ],
                            rest: None,
                        },
                        guard: None,
                        body: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("greet".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("n".to_string()))],
                        }),
                    },
                    MatchClause {
                        pattern: MatchPattern::Map {
                            entries: vec![
                                crate::ast::MapMatchEntry {
                                    key: MapKey::Keyword(Keyword("type".to_string())),
                                    pattern: Box::new(MatchPattern::Literal(Literal::String(
                                        "admin".to_string()
                                    ))),
                                }
                            ],
                            rest: None,
                        },
                        guard: None,                        body: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("admin-panel".to_string()))),
                            arguments: vec![],
                        }),
                    },
                ],
            })
        );
    }    #[test]
    fn test_parse_match_with_multiple_body_expressions() {        // Test multiple body expressions per pattern
        assert_expr_parses_to!(            r#"(match my-val :case1 (do (expr1) (expr2)) _ (default-expr))"#,
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("my-val".to_string()))),                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Literal(Literal::Keyword(Keyword("case1".to_string()))),
                        guard: None,
                        body: Box::new(Expression::Do(DoExpr {
                            expressions: vec![
                                Expression::FunctionCall {
                                    callee: Box::new(Expression::Symbol(Symbol("expr1".to_string()))),
                                    arguments: vec![],
                                },
                                Expression::FunctionCall {
                                    callee: Box::new(Expression::Symbol(Symbol("expr2".to_string()))),
                                    arguments: vec![],
                                },
                            ],
                        })),
                    },                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("default-expr".to_string()))),
                            arguments: vec![],
                        }),                    },
                ],
            })
        );
    }    #[test]
    fn test_parse_discover_agents_basic() {
        // Test basic discover-agents with criteria only
        let parse_result = RTFSParser::parse(Rule::expression, r#"(discover-agents {:capabilities ["nlp" "web"]})"#);
        assert!(parse_result.is_ok(), "Failed to parse: {:?}", parse_result.err());
        
        let pair = parse_result.unwrap().next().unwrap();
        let expr = build_expression(pair).unwrap();
        
        // Verify it's a DiscoverAgents expression
        match expr {
            Expression::DiscoverAgents(discover_expr) => {
                // Verify criteria is a Map
                match discover_expr.criteria.as_ref() {
                    Expression::Map(map) => {
                        assert!(map.contains_key(&MapKey::Keyword(Keyword("capabilities".to_string()))));
                    },
                    _ => panic!("Expected criteria to be a Map"),
                }
                // Verify options is None
                assert!(discover_expr.options.is_none());
            },
            _ => panic!("Expected DiscoverAgents expression, got: {:?}", expr),
        }
    }

    #[test]
    fn test_parse_discover_agents_with_options() {
        // Test discover-agents with both criteria and options
        let parse_result = RTFSParser::parse(Rule::expression, r#"(discover-agents {:type "llm"} {:timeout 5000})"#);
        assert!(parse_result.is_ok(), "Failed to parse: {:?}", parse_result.err());
        
        let pair = parse_result.unwrap().next().unwrap();
        let expr = build_expression(pair).unwrap();
        
        // Verify it's a DiscoverAgents expression
        match expr {
            Expression::DiscoverAgents(discover_expr) => {
                // Verify criteria is a Map with correct key
                match discover_expr.criteria.as_ref() {
                    Expression::Map(map) => {
                        assert!(map.contains_key(&MapKey::Keyword(Keyword("type".to_string()))));
                    },
                    _ => panic!("Expected criteria to be a Map"),
                }
                // Verify options is Some and contains timeout
                match discover_expr.options.as_ref() {
                    Some(options) => {
                        match options.as_ref() {
                            Expression::Map(map) => {
                                assert!(map.contains_key(&MapKey::Keyword(Keyword("timeout".to_string()))));
                            },
                            _ => panic!("Expected options to be a Map"),
                        }
                    },
                    None => panic!("Expected options to be Some"),
                }
            },
            _ => panic!("Expected DiscoverAgents expression, got: {:?}", expr),
        }
    }

    #[test]
    fn test_parse_discover_agents_complex_criteria() {
        // Test discover-agents with complex criteria
        let parse_result = RTFSParser::parse(Rule::expression, r#"(discover-agents {:capabilities ["nlp" "vision"] :model-size "large"})"#);
        assert!(parse_result.is_ok(), "Failed to parse: {:?}", parse_result.err());
        
        let pair = parse_result.unwrap().next().unwrap();
        let expr = build_expression(pair).unwrap();
        
        // Verify it's a DiscoverAgents expression
        match expr {
            Expression::DiscoverAgents(discover_expr) => {
                // Verify criteria is a Map with multiple keys
                match discover_expr.criteria.as_ref() {
                    Expression::Map(map) => {
                        assert!(map.contains_key(&MapKey::Keyword(Keyword("capabilities".to_string()))));
                        assert!(map.contains_key(&MapKey::Keyword(Keyword("model-size".to_string()))));
                        assert_eq!(map.len(), 2);
                    },
                    _ => panic!("Expected criteria to be a Map"),
                }
                // Verify options is None
                assert!(discover_expr.options.is_none());
            },
            _ => panic!("Expected DiscoverAgents expression, got: {:?}", expr),
        }
    }

    #[test]
    fn test_parse_discover_agents_with_whitespace() {
        // Test discover-agents with various whitespace
        let parse_result = RTFSParser::parse(Rule::expression, r#"(discover-agents
                {:type "assistant"}
                {:timeout 3000}
            )"#);
        assert!(parse_result.is_ok(), "Failed to parse: {:?}", parse_result.err());
        
        let pair = parse_result.unwrap().next().unwrap();
        let expr = build_expression(pair).unwrap();
        
        // Verify it's a DiscoverAgents expression
        match expr {
            Expression::DiscoverAgents(discover_expr) => {
                // Verify criteria is a Map
                match discover_expr.criteria.as_ref() {
                    Expression::Map(map) => {
                        assert!(map.contains_key(&MapKey::Keyword(Keyword("type".to_string()))));
                    },
                    _ => panic!("Expected criteria to be a Map"),
                }
                // Verify options is Some
                assert!(discover_expr.options.is_some());
            },
            _ => panic!("Expected DiscoverAgents expression, got: {:?}", expr),
        }
    }
}
