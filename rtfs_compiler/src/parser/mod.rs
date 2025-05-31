use pest::iterators::{Pair, Pairs}; // Added Pairs
use pest::Parser;

// Define a custom error type for parsing
#[derive(Debug)]
pub enum PestParseError {
    UnexpectedRule {
        expected: String,
        found: String,
        rule_text: String,
    },
    MissingToken(String),
    InvalidInput(String),          // Added
    UnsupportedRule(String),       // Added
    InvalidLiteral(String),        // Added
    InvalidEscapeSequence(String), // Added
    CustomError(String),
    PestError(pest::error::Error<Rule>), // For errors from Pest itself
}

impl From<pest::error::Error<Rule>> for PestParseError {
    fn from(err: pest::error::Error<Rule>) -> Self {
        PestParseError::PestError(err)
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
// Removed unused build_keyword, build_literal, build_map_key
use common::build_symbol;
use expressions::{build_expression, build_map}; // Added build_map
use special_forms::{build_def_expr, build_defn_expr};
use utils::unescape; // Added def/defn builders

// Define the parser struct using the grammar file
#[derive(pest_derive::Parser)]
#[grammar = "rtfs.pest"] // Path relative to src/
struct RTFSParser;

// Helper to skip whitespace and comments in a Pairs iterator
fn next_significant<'a>(pairs: &mut Pairs<'a, Rule>) -> Option<Pair<'a, Rule>> {
    pairs.find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
}

// --- Main Parsing Function ---

// pub fn parse(input: &str) -> Result<Vec<TopLevel>, pest::error::Error<Rule>> {
pub fn parse(input: &str) -> Result<Vec<TopLevel>, PestParseError> {
    // MODIFIED error type
    let pairs = RTFSParser::parse(Rule::program, input).map_err(PestParseError::from)?; // MODIFIED to map error
                                                                                        // Program contains SOI ~ (task_definition | module_definition | expression)* ~ EOI
                                                                                        // The `pairs` variable is an iterator over the content matched by Rule::program.
                                                                                        // We need its single inner item (which should be the sequence inside program)
                                                                                        // then iterate over *that* sequence's inner items.
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
        Rule::module_definition => build_module_definition(pair).map(TopLevel::Module), // MODIFIED NEW

        // Import definition should only appear inside a module, handle within build_module_definition
        Rule::import_definition => {
            // panic!("Import definition found outside of a module context") // OLD
            Err(PestParseError::CustomError( // NEW
                "Import definition found outside of a module context".to_string(),
            ))
        }

        // Handle unexpected rules at this level
        // rule => unimplemented!( // OLD
        //     "build_ast encountered unexpected top-level rule: {:?}, content: \'{}\'",
        //     rule,
        //     pair.as_str()
        // ),
        rule => Err(PestParseError::CustomError(format!( // NEW
            "build_ast encountered unexpected top-level rule: {:?}, content: '{}'",
            rule,
            pair.as_str()
        ))),
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

// Helper function to build export options
// Expected pairs: inner of export_option rule (exports_keyword, export_symbols_vec)
fn build_export_option(mut pairs: Pairs<Rule>) -> Result<Vec<Symbol>, PestParseError> {
    let exports_keyword_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError("Expected :exports keyword in export_option".to_string())
    })?;
    if exports_keyword_pair.as_rule() != Rule::exports_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: ":exports keyword".to_string(),
            found: format!("{:?}", exports_keyword_pair.as_rule()),
            rule_text: exports_keyword_pair.as_str().to_string(),
        });
    }

    let symbols_vec_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError("Expected symbols vector in export_option".to_string())
    })?;
    if symbols_vec_pair.as_rule() != Rule::export_symbols_vec {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbols vector".to_string(),
            found: format!("{:?}", symbols_vec_pair.as_rule()),
            rule_text: symbols_vec_pair.as_str().to_string(),
        });
    }

    symbols_vec_pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::symbol)
        // .map(|p| Ok(build_symbol(p))) // build_symbol doesn't return Result, so wrap Ok // OLD COMMENT & CODE
        // .collect() // OLD
        .map(build_symbol) // NEW - build_symbol now returns Result
        .collect::<Result<Vec<Symbol>, PestParseError>>() // NEW - collect into Result
}

// module_definition =  { "(" ~ module_keyword ~ symbol ~ export_option? ~ definition* ~ ")" }
// fn build_module_definition(pair: Pair<Rule>) -> ModuleDefinition { // Old signature
fn build_module_definition(pair: Pair<Rule>) -> Result<ModuleDefinition, PestParseError> {
    // New signature
    // assert_eq!(pair.as_rule(), Rule::module_definition); // Already asserted by caller build_ast
    let mut inner_pairs = pair.into_inner(); // Consumes the Rule::module_definition pair itself

    // 1. module_keyword
    let module_keyword_pair = next_significant(&mut inner_pairs).ok_or_else(|| {
        PestParseError::CustomError("Module definition missing module keyword".to_string())
    })?;
    if module_keyword_pair.as_rule() != Rule::module_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "module_keyword".to_string(),
            found: format!("{:?}", module_keyword_pair.as_rule()),
            rule_text: module_keyword_pair.as_str().to_string(),
        });
    }

    // 2. Name (symbol)
    let name_pair = next_significant(&mut inner_pairs).ok_or_else(|| {
        PestParseError::CustomError("Module definition requires a name".to_string())
    })?;
    // As per grammar: module_definition = { "(" ~ module_keyword ~ symbol ...
    if name_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol for module name".to_string(),
            found: format!("{:?}", name_pair.as_rule()),
            rule_text: name_pair.as_str().to_string(),
        });
    }
    // let name = build_symbol(name_pair); // OLD
    let name = build_symbol(name_pair)?; // NEW

    let mut exports = None;
    let mut definitions = Vec::new();

    // Use peekable to check for optional export_option
    let mut remaining_module_parts = inner_pairs.peekable();

    // 3. Optional export_option
    if let Some(peeked_part) = remaining_module_parts.peek() {
        if peeked_part.as_rule() == Rule::export_option {
            let export_pair = remaining_module_parts.next().unwrap(); // Consume it
            exports = Some(build_export_option(export_pair.into_inner())?);
        }
    }

    // 4. Definitions (def_expr | defn_expr | import_definition)*
    for def_candidate_pair in remaining_module_parts {
        // Skip whitespace and comments if they are passed (though next_significant in a loop would be better if they could be mixed)
        // The current loop structure assumes `remaining_module_parts` only yields significant tokens.
        match def_candidate_pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT | Rule::EOI => continue, // EOI might appear if it\'s the last thing
            Rule::def_expr => {
                // let def_node = build_def_expr(def_candidate_pair.into_inner()); // OLD
                let def_node = build_def_expr(def_candidate_pair.into_inner())?; // NEW
                definitions.push(ModuleLevelDefinition::Def(def_node));
            }
            Rule::defn_expr => {
                eprintln!(
                    "[build_module_definition] Calling build_defn_expr for: rule={:?}, str='{}'",
                    def_candidate_pair.as_rule(),
                    def_candidate_pair.as_str()
                );
                // let defn_node = build_defn_expr(def_candidate_pair.into_inner()); // OLD
                let defn_node = build_defn_expr(def_candidate_pair.into_inner())?; // NEW
                definitions.push(ModuleLevelDefinition::Defn(defn_node));
            }
            Rule::import_definition => {
                let import_node = build_import_definition(def_candidate_pair.into_inner())?;
                definitions.push(ModuleLevelDefinition::Import(import_node));
            }
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "def_expr, defn_expr, or import_definition".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: def_candidate_pair.as_str().to_string(),
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
fn build_import_definition(mut pairs: Pairs<Rule>) -> Result<ImportDefinition, PestParseError> {
    // New signature, takes inner pairs
    // assert_eq!(pair.as_rule(), Rule::import_definition); // Asserted by caller
    // let mut inner = pair.into_inner(); // Caller now passes inner pairs

    // 1. import_keyword
    let import_keyword_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError("Import definition missing import keyword".to_string())
    })?;
    if import_keyword_pair.as_rule() != Rule::import_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "import_keyword".to_string(),
            found: format!("{:?}", import_keyword_pair.as_rule()),
            rule_text: import_keyword_pair.as_str().to_string(),
        });
    }

    // 2. Module Name
    let module_name_pair = next_significant(&mut pairs).ok_or_else(|| {
        PestParseError::CustomError("Import definition requires a module name".to_string())
    })?;
    if !(module_name_pair.as_rule() == Rule::namespaced_identifier
        || module_name_pair.as_rule() == Rule::symbol)
    {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol or namespaced_identifier for import module name".to_string(),
            found: format!("{:?}", module_name_pair.as_rule()),
            rule_text: module_name_pair.as_str().to_string(),
        });
    }
    // let module_name = build_symbol(module_name_pair); // OLD
    let module_name = build_symbol(module_name_pair)?; // NEW

    let mut alias = None;
    let mut only = None;

    // Parse import_option rules (which contain ":as symbol" or ":only [symbols]")
    while let Some(option_pair) = next_significant(&mut pairs) {
        if option_pair.as_rule() == Rule::import_option {
            let option_text = option_pair.as_str();
            let option_inner = option_pair.into_inner();

            // The first element tells us which type of option this is
            if option_text.starts_with(":as") {
                // Skip the ":as" part and get the symbol
                // The import_option rule is ":as" ~ symbol, so we need to find the symbol
                let mut found_symbol = false;
                for inner_pair in option_inner {
                    if inner_pair.as_rule() == Rule::symbol {
                        // alias = Some(build_symbol(inner_pair)); // OLD
                        alias = Some(build_symbol(inner_pair)?); // NEW
                        found_symbol = true;
                        break;
                    }
                }
                if !found_symbol {
                    return Err(PestParseError::CustomError(
                        "Import :as option missing symbol".to_string(),
                    ));
                }
            } else if option_text.starts_with(":only") {
                // The import_option rule is ":only" ~ "[" ~ symbol+ ~ "]"
                // We need to find the symbols within the brackets
                // let mut symbols = Vec::new(); // OLD
                // for inner_pair in option_inner { // OLD
                //     if inner_pair.as_rule() == Rule::symbol { // OLD
                //         symbols.push(build_symbol(inner_pair)); // OLD - build_symbol used to not return Result or was handled differently
                //     } // OLD
                // } // OLD

                let collected_symbols: Result<Vec<Symbol>, PestParseError> =
                    option_inner // NEW
                        .filter(|p| p.as_rule() == Rule::symbol) // Ensure we only try to build symbols from symbol rules
                        .map(build_symbol) // build_symbol returns Result<Symbol, PestParseError>
                        .collect();
                let symbols = collected_symbols?; // NEW - propagate error if collection failed

                if symbols.is_empty() {
                    return Err(PestParseError::CustomError(
                        "Import :only option requires at least one symbol".to_string(),
                    ));
                }
                only = Some(symbols); // NEW - symbols is now Vec<Symbol>
            } else {
                return Err(PestParseError::CustomError(format!(
                    "Unknown import option: {}",
                    option_text
                )));
            }
        } else {
            return Err(PestParseError::CustomError(format!(
                "Expected import_option, found: {:?}",
                option_pair.as_rule()
            )));
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
        // DoExpr, // Removed unused import
        Expression,
        FnExpr,
        // IfExpr, // Removed unused import
        ImportDefinition,
        Keyword,
        LetBinding,
        LetExpr,
        Literal,
        // LogStepExpr, // Removed unused import
        MapDestructuringEntry, // Changed from MapPatternEntry
        MapKey,
        MapMatchEntry, // Added for match map patterns
        MapTypeEntry,
        MatchClause,
        MatchExpr,
        MatchPattern, // Added for match patterns
        ModuleDefinition,
        ModuleLevelDefinition,
        ParallelBinding,
        ParallelExpr,
        ParamDef,
        ParamType,
        Pattern,
        PrimitiveType, // Added for TypeExpr::Primitive
        Symbol,
        TaskDefinition,
        TopLevel,
        TryCatchExpr,
        TypeExpr,
        WithResourceExpr,
    };
    use crate::parser::types::build_type_expr;
    use std::collections::HashMap; // Added HashMap for map tests

    // Helper macro for asserting expression parsing
    macro_rules! assert_expr_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = RTFSParser::parse(Rule::expression, $input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse expression (RTFSParser::parse):\\nInput: {:?}\\nError: {:?}",
                $input,
                parse_result.err().unwrap()
            );
            let expr_pair = parse_result.unwrap().next().unwrap();
            let expr_pair_str = expr_pair.as_str().to_string(); // Clone the string before moving expr_pair
            let ast_result = expressions::build_expression(expr_pair); // This returns Result<Expression, PestParseError>
            assert!(
                ast_result.is_ok(),
                "Failed to build expression (expressions::build_expression):\\nInput: {:?}\\nSource pair: {:?}\\nError: {:?}",
                $input,
                expr_pair_str,
                ast_result.err().unwrap()
            );
            let ast = ast_result.unwrap(); // Now ast is an Expression
            assert_eq!(
                ast, $expected, // $expected is a direct Expression
                "Expression AST mismatch for input: {:?}\\nExpected: {:#?}\\nActual: {:#?}",
                $input, $expected, ast
            );
        };
    }

    // Helper macro for asserting top-level parsing
    macro_rules! assert_program_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = parse($input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse program:\nInput: {:?}\nError: {:?}",
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
        assert_expr_parses_to!("123", Expression::Literal(Literal::Integer(123))); // MODIFIED
        assert_expr_parses_to!("-45", Expression::Literal(Literal::Integer(-45))); // MODIFIED
        assert_expr_parses_to!("1.23", Expression::Literal(Literal::Float(1.23))); // MODIFIED
        assert_expr_parses_to!("-0.5", Expression::Literal(Literal::Float(-0.5))); // MODIFIED
        assert_expr_parses_to!(
            "\\\"hello\\\"",                                           // RTFS source: "hello"
            Expression::Literal(Literal::String("hello".to_string()))  // MODIFIED
        );
        assert_expr_parses_to!(
            "\\\"hello\\\\\\\\world\\\\n\\\"", // RTFS source: "hello\\world\\n"
            Expression::Literal(Literal::String("hello\\\\world\\n".to_string()))  // MODIFIED
        );
        assert_expr_parses_to!("true", Expression::Literal(Literal::Boolean(true))); // MODIFIED
        assert_expr_parses_to!("false", Expression::Literal(Literal::Boolean(false))); // MODIFIED
        assert_expr_parses_to!("nil", Expression::Literal(Literal::Nil)); // MODIFIED
    }

    #[test]
    fn test_parse_symbol_keyword() {
        assert_expr_parses_to!(
            "my-var",
            Expression::Symbol(Symbol("my-var".to_string())) // MODIFIED
        );
        assert_expr_parses_to!(
            "ns/my-var",
            Expression::Symbol(Symbol("ns/my-var".to_string())) // MODIFIED
        );
        assert_expr_parses_to!(
            ":my-key",
            Expression::Literal(Literal::Keyword(Keyword("my-key".to_string()))) // MODIFIED
        );
    }

    #[test]
    fn test_parse_collections() {
        // Vector
        assert_expr_parses_to!(
            "[1 2 \"three\"]",
            Expression::Vector(vec![
                // MODIFIED
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
                Expression::Literal(Literal::String("three".to_string())),
            ])
        );
        assert_expr_parses_to!("[]", Expression::Vector(vec![])); // MODIFIED

        // List (Function Call heuristic)
        assert_expr_parses_to!(
            "(a b c)",
            Expression::FunctionCall {
                // MODIFIED
                function: Box::new(Expression::Symbol(Symbol("a".to_string()))),
                arguments: vec![
                    Expression::Symbol(Symbol("b".to_string())),
                    Expression::Symbol(Symbol("c".to_string())),
                ]
            }
        );
        // Empty list is still a list
        assert_expr_parses_to!("()", Expression::List(vec![])); // MODIFIED
                                                                // List starting with non-symbol is a list
        assert_expr_parses_to!(
            "(1 2 3)",
            Expression::List(vec![
                // MODIFIED
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
            "{ :a 1 \"b\" true }",
            Expression::Map(expected_map.clone()) // MODIFIED
        );
        assert_expr_parses_to!("{}", Expression::Map(HashMap::new())); // MODIFIED

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
            "{0 \"zero\" :a 1}",
            Expression::Map(map_with_int_key.clone()) // MODIFIED
        );
    }

    #[test]
    fn test_parse_def() {
        assert_expr_parses_to!(
            "(def x 1)",
            Expression::Def(Box::new(DefExpr {
                // MODIFIED
                symbol: Symbol("x".to_string()),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(1))),
            }))
        );
        assert_expr_parses_to!(
            "(def y MyType \"value\")",
            Expression::Def(Box::new(DefExpr {
                // MODIFIED
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
            "(let [x 1 y \"hi\"] (+ x 1))",
            Expression::Let(LetExpr {
                // MODIFIED
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
                    function: Box::new(Expression::Symbol(Symbol("+".to_string()))),
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
                // MODIFIED
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
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("do".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("a".to_string())),
                        Expression::Symbol(Symbol("b".to_string())),
                        Expression::Symbol(Symbol("rest".to_string())),
                        Expression::Symbol(Symbol("all-v".to_string())),
                        Expression::Symbol(Symbol("x".to_string())),
                    ],
                }],
            })
        );
        // Let with map destructuring
        assert_expr_parses_to!(
            "(let [{:key1 val1 :keys [s1 s2] \"str-key\" val2 & r :as all-m} my-map] (do val1 s1 s2 val2 r all-m))",
            Expression::Let(LetExpr { // MODIFIED
                bindings: vec![LetBinding {
                    pattern: Pattern::MapDestructuring {
                        entries: vec![
                            MapDestructuringEntry::KeyBinding {
                                key: MapKey::Keyword(Keyword("key1".to_string())),
                                pattern: Box::new(Pattern::Symbol(Symbol("val1".to_string()))),
                            },
                            MapDestructuringEntry::Keys(vec![
                                Symbol("s1".to_string()),
                                Symbol("s2".to_string()),
                            ]),
                            MapDestructuringEntry::KeyBinding {
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
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("do".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("val1".to_string())),
                        Expression::Symbol(Symbol("s1".to_string())),
                        Expression::Symbol(Symbol("s2".to_string())),
                        Expression::Symbol(Symbol("val2".to_string())),
                        Expression::Symbol(Symbol("r".to_string())),
                        Expression::Symbol(Symbol("all-m".to_string())),
                    ],
                }],
            })
        );
        // Let with wildcard
        assert_expr_parses_to!(
            "(let [_ 1 y 2] y)",
            Expression::Let(LetExpr {
                // MODIFIED
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
            "(def x 1) \\n ; comment \\n \"hello\"",
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
                timestamp: None, // Not included in input
                intent: Some(Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("generate-code".to_string()))),
                    arguments: vec![Expression::Literal(Literal::String(
                        "Create a button".to_string()
                    ))],
                }),
                contracts: Some(Expression::Map(contracts_map)),
                plan: Some(Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("step-1".to_string()))),
                    arguments: vec![Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("step-2".to_string()))),
                        arguments: vec![],
                    }],
                }),
                execution_trace: Some(Expression::Vector(vec![Expression::Map(trace_map)])),
                metadata: None, // Added to match AST
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
          (defn public-fn [x :ParamType & rest-args :RestType] :ReturnType ; Params, variadic, return type
            (other/do-something x private-val rest-args)) ; Body expression
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
                        // Changed to ParamDef
                        pattern: Pattern::Symbol(Symbol("rest-args".to_string())),
                        type_annotation: Some(TypeExpr::Alias(Symbol("RestType".to_string()))),
                    }),
                    return_type: Some(TypeExpr::Alias(Symbol("ReturnType".to_string()))),
                    body: vec![Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol(
                            "other/do-something".to_string(),
                        ))),
                        arguments: vec![
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Symbol(Symbol("private-val".to_string())),
                            Expression::Symbol(Symbol("rest-args".to_string())),
                        ],
                    }],
                }),
            ],
        })];
        let parse_result = parse(input);
        assert!(
            parse_result.is_ok(),
            "Failed to parse module definition: {:?}",
            parse_result.err()
        );
        let actual = parse_result.unwrap();
        use std::fs::File;
        use std::io::Write;
        // Write expected and actual ASTs to files for easy diffing
        let mut expected_file = File::create("expected_ast.txt").unwrap();
        let mut actual_file = File::create("actual_ast.txt").unwrap();
        writeln!(expected_file, "{:#?}", expected).unwrap();
        writeln!(actual_file, "{:#?}", actual).unwrap();
        println!("\\n==================== AST DIFF HELP ====================");
        println!("Expected AST written to: expected_ast.txt");
        println!("Actual AST   written to: actual_ast.txt");
        println!("To see a diff, run: diff -u expected_ast.txt actual_ast.txt");
        println!("(Or use your favorite diff tool)");
        println!("======================================================\\n");
        println!("Expected AST: {:#?}", expected);
        println!("Actual AST: {:#?}\\n", actual);
        assert_eq!(actual, expected, "Module definition AST mismatch");
    }

    // --- Tests for New Special Forms ---

    #[test]
    fn test_parse_parallel() {
        assert_expr_parses_to!(
            "(parallel [a (f 1)] [b :SomeType (g 2)])",
            Expression::Parallel(ParallelExpr {
                // MODIFIED
                bindings: vec![
                    ParallelBinding {
                        symbol: Symbol("a".to_string()),
                        type_annotation: None,
                        expression: Box::new(Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("f".to_string()))),
                            arguments: vec![Expression::Literal(Literal::Integer(1))],
                        }),
                    },
                    ParallelBinding {
                        symbol: Symbol("b".to_string()),
                        type_annotation: Some(TypeExpr::Alias(Symbol("SomeType".to_string()))), // Assuming Alias for now
                        expression: Box::new(Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("g".to_string()))),
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
            "(with-resource [res :ResourceType (init-res)] (use res))",
            Expression::WithResource(WithResourceExpr {
                // MODIFIED
                resource_symbol: Symbol("res".to_string()),
                resource_type: TypeExpr::Alias(Symbol("ResourceType".to_string())),
                resource_init: Box::new(Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("init-res".to_string()))),
                    arguments: vec![],
                }),
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("use".to_string()))),
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
                // MODIFIED
                try_body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("dangerous-op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("Error".to_string())),
                        binding: Symbol("e".to_string()),
                        body: vec![Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("e".to_string()))],
                        }],
                    },
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("OtherError".to_string())),
                        binding: Symbol("oe".to_string()),
                        body: vec![Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("log".to_string()))),
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
                // MODIFIED
                try_body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![CatchClause {
                    pattern: CatchPattern::Keyword(Keyword("E".to_string())),
                    binding: Symbol("e".to_string()),
                    body: vec![Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                        arguments: vec![Expression::Symbol(Symbol("e".to_string()))],
                    }],
                }],
                finally_body: Some(vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("cleanup".to_string()))),
                    arguments: vec![],
                }]),
            })
        );

        // Try-finally (no catch)
        assert_expr_parses_to!(
            "(try (main-op) (finally (always-run)))",
            Expression::TryCatch(TryCatchExpr {
                // MODIFIED
                try_body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("main-op".to_string()))),
                    arguments: vec![],
                }],
                catch_clauses: vec![], // Empty catch clauses
                finally_body: Some(vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("always-run".to_string()))),
                    arguments: vec![],
                }]),
            })
        );
    }

    #[test]
    fn test_parse_match() {
        assert_expr_parses_to!(
            "(match my-val 1 \"one\" [2 3] \"two-three\" _ \"default\")",
            Expression::Match(Box::new(MatchExpr {
                // MODIFIED
                expression: Box::new(Expression::Symbol(Symbol("my-val".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Literal(Literal::Integer(1)),
                        guard: None,
                        body: vec![Expression::Literal(Literal::String("one".to_string()))],
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
                        body: vec![Expression::Literal(Literal::String(
                            "two-three".to_string()
                        ))],
                    },
                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: vec![Expression::Literal(Literal::String("default".to_string()))],
                    },
                ],
            }))
        );

        // Match with guard
        assert_expr_parses_to!(
            "(match x [a b] (when (> a b)) (list a b) _ nil)",
            Expression::Match(Box::new(MatchExpr {
                // MODIFIED
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
                            function: Box::new(Expression::Symbol(Symbol(">".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("a".to_string())),
                                Expression::Symbol(Symbol("b".to_string())),
                            ],
                        })),
                        body: vec![Expression::List(vec![
                            // Changed to Expression::List
                            Expression::Symbol(Symbol("a".to_string())),
                            Expression::Symbol(Symbol("b".to_string())),
                        ])],
                    },
                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: vec![Expression::Literal(Literal::Nil)],
                    },
                ],
            }))
        );

        // Match with map pattern
        assert_expr_parses_to!(
            "(match data {:type \"user\" :name n} (greet n) { :type \"admin\" } (admin-panel))",
            Expression::Match(Box::new(MatchExpr {
                // MODIFIED
                expression: Box::new(Expression::Symbol(Symbol("data".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Map {
                            entries: vec![
                                MapMatchEntry {
                                    key: MapKey::Keyword(Keyword("type".to_string())),
                                    pattern: Box::new(MatchPattern::Literal(Literal::String(
                                        "user".to_string()
                                    ))),
                                },
                                MapMatchEntry {
                                    key: MapKey::Keyword(Keyword("name".to_string())),
                                    pattern: Box::new(MatchPattern::Symbol(Symbol(
                                        "n".to_string()
                                    ))),
                                },
                            ],
                            rest: None,
                        },
                        guard: None,
                        body: vec![Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("greet".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("n".to_string()))],
                        }],
                    },
                    MatchClause {
                        pattern: MatchPattern::Map {
                            entries: vec![MapMatchEntry {
                                key: MapKey::Keyword(Keyword("type".to_string())),
                                pattern: Box::new(MatchPattern::Literal(Literal::String(
                                    "admin".to_string()
                                ))),
                            },],
                            rest: None,
                        },
                        guard: None,
                        body: vec![Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol(
                                "admin-panel".to_string()
                            ))),
                            arguments: vec![],
                        }],
                    },
                ],
            }))
        );
    }

    // TODO: Add tests for fn, defn once pattern/type parsing is robust
    // TODO: Add tests for comments and whitespace handling within structures
    // TODO: Add tests for complex patterns (map/vector destructuring in let, fn, match)
    // TODO: Add tests for complex type expressions

    // --- Tests for fn ---
    #[test]
    fn test_parse_fn() {
        // Simple fn
        assert_expr_parses_to!(
            "(fn [x y] (+ x y))",
            Expression::Fn(FnExpr {
                // MODIFIED
                params: vec![
                    ParamDef {
                        pattern: Pattern::Symbol(Symbol("x".to_string())),
                        type_annotation: None
                    },
                    ParamDef {
                        pattern: Pattern::Symbol(Symbol("y".to_string())),
                        type_annotation: None
                    },
                ],
                variadic_param: None,
                return_type: None,
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("x".to_string())),
                        Expression::Symbol(Symbol("y".to_string())),
                    ],
                }],
            })
        );

        // Fn with type annotations and variadic param
        assert_expr_parses_to!(
            "(fn [name :String & rest :Any] :String (str name (join rest)))",
            Expression::Fn(FnExpr {
                // MODIFIED
                params: vec![ParamDef {
                    pattern: Pattern::Symbol(Symbol("name".to_string())),
                    type_annotation: Some(TypeExpr::Alias(Symbol("String".to_string())))
                },],
                variadic_param: Some(ParamDef {
                    pattern: Pattern::Symbol(Symbol("rest".to_string())),
                    type_annotation: Some(TypeExpr::Alias(Symbol("Any".to_string())))
                }),
                return_type: Some(TypeExpr::Alias(Symbol("String".to_string()))),
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("str".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("name".to_string())),
                        Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol("join".to_string()))),
                            arguments: vec![Expression::Symbol(Symbol("rest".to_string()))],
                        }
                    ],
                }],
            })
        );

        // Fn with destructuring in params
        assert_expr_parses_to!(
            "(fn [{:x x-val} [y z]] (+ x-val y z))",
            Expression::Fn(FnExpr {
                // MODIFIED
                params: vec![
                    ParamDef {
                        pattern: Pattern::MapDestructuring {
                            entries: vec![MapDestructuringEntry::KeyBinding {
                                key: MapKey::Keyword(Keyword("x".to_string())),
                                pattern: Box::new(Pattern::Symbol(Symbol("x-val".to_string())))
                            }],
                            rest: None,
                            as_symbol: None,
                        },
                        type_annotation: None
                    },
                    ParamDef {
                        pattern: Pattern::VectorDestructuring {
                            elements: vec![
                                Pattern::Symbol(Symbol("y".to_string())),
                                Pattern::Symbol(Symbol("z".to_string())),
                            ],
                            rest: None,
                            as_symbol: None,
                        },
                        type_annotation: None
                    },
                ],
                variadic_param: None,
                return_type: None,
                body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Symbol(Symbol("x-val".to_string())),
                        Expression::Symbol(Symbol("y".to_string())),
                        Expression::Symbol(Symbol("z".to_string())),
                    ],
                }],
            })
        );
    }

    // Helper macro for asserting type expression parsing
    macro_rules! assert_type_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = RTFSParser::parse(Rule::type_expr, $input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse type expression:\nInput: {:?}\nError: {:?}",
                $input,
                parse_result.err().unwrap()
            );
            let type_pair = parse_result.unwrap().next().unwrap();
            // build_type_expr now returns Result<TypeExpr, PestParseError>
            let ast_result = build_type_expr(type_pair);
            assert!(
                ast_result.is_ok(),
                "Failed to build TypeExpr AST:\nInput: {:?}\nError: {:?}",
                $input,
                ast_result.as_ref().err() // Use as_ref() to borrow for err()
            );
            let ast = ast_result.unwrap();
            assert_eq!(
                ast, $expected,
                "TypeExpr AST mismatch for input: {:?}\nExpected: {:#?}\nActual: {:#?}",
                $input, $expected, ast
            );
        };
    }

    #[test]
    fn test_parse_type_expressions() {
        // Primitive types
        assert_type_parses_to!(":int", TypeExpr::Primitive(PrimitiveType::Int));
        assert_type_parses_to!(":string", TypeExpr::Primitive(PrimitiveType::String));
        assert_type_parses_to!(":any", TypeExpr::Any);
        assert_type_parses_to!(":never", TypeExpr::Never);

        // Symbol as type alias
        assert_type_parses_to!(
            "MyCustomType",
            TypeExpr::Alias(Symbol("MyCustomType".to_string()))
        );
        assert_type_parses_to!(
            "my.namespace/Type",
            TypeExpr::Alias(Symbol("my.namespace/Type".to_string()))
        );

        // Vector type
        assert_type_parses_to!(
            "[:vector :int]",
            TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int)))
        );
        assert_type_parses_to!(
            "[:vector [:vector :string]]",
            TypeExpr::Vector(Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(
                PrimitiveType::String
            )))))
        );

        // Map type
        assert_type_parses_to!(
            "[:map [:key1 :int] [:key2? :string] [:* :any]]",
            TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword("key1".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                        optional: false,
                    },
                    MapTypeEntry {
                        key: Keyword("key2".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                        optional: true,
                    },
                ],
                wildcard: Some(Box::new(TypeExpr::Any)),
            }
        );
        assert_type_parses_to!(
            "[:map]",
            TypeExpr::Map {
                entries: vec![],
                wildcard: None
            }
        );

        // Function type
        assert_type_parses_to!(
            "[:=> [:int :string] :bool]", // Simple params, return
            TypeExpr::Function {
                param_types: vec![
                    // Changed from params
                    ParamType::Simple(Box::new(TypeExpr::Primitive(PrimitiveType::Int))),
                    ParamType::Simple(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
                ],
                variadic_param_type: None, // Changed from variadic
                return_type: Box::new(TypeExpr::Primitive(PrimitiveType::Bool)),
            }
        );
        assert_type_parses_to!(
            "[:=> [:A & :B] :C]", // Variadic
            TypeExpr::Function {
                param_types: vec![ParamType::Simple(Box::new(TypeExpr::Alias(Symbol(
                    "A".to_string()
                ))))], // Changed from params
                variadic_param_type: Some(Box::new(TypeExpr::Alias(Symbol("B".to_string())))), // Changed from variadic
                return_type: Box::new(TypeExpr::Alias(Symbol("C".to_string()))),
            }
        );
        assert_type_parses_to!(
            "[:=> [] :void]", // No params
            TypeExpr::Function {
                param_types: vec![],       // Changed from params
                variadic_param_type: None, // Changed from variadic
                return_type: Box::new(TypeExpr::Primitive(PrimitiveType::Custom(Keyword(
                    "void".to_string()
                )))), // Assuming :void is a custom primitive
            }
        );

        // Resource type
        assert_type_parses_to!(
            "[:resource my.resource/Handle]",
            TypeExpr::Resource(Symbol("my.resource/Handle".to_string()))
        );

        // Union type
        assert_type_parses_to!(
            "[:or :int :string :nil]",
            TypeExpr::Union(vec![
                TypeExpr::Primitive(PrimitiveType::Int),
                TypeExpr::Primitive(PrimitiveType::String),
                TypeExpr::Primitive(PrimitiveType::Nil),
            ])
        );

        // Intersection type
        assert_type_parses_to!(
            "[:and HasName HasAge]",
            TypeExpr::Intersection(vec![
                TypeExpr::Alias(Symbol("HasName".to_string())),
                TypeExpr::Alias(Symbol("HasAge".to_string())),
            ])
        );

        // Literal type
        assert_type_parses_to!("[:val 123]", TypeExpr::Literal(Literal::Integer(123)));
        assert_type_parses_to!(
            "[:val \"hello\"]", // Corrected line
            TypeExpr::Literal(Literal::String("hello".to_string()))
        );
        assert_type_parses_to!("[:val true]", TypeExpr::Literal(Literal::Boolean(true)));
    }

    // TODO: Add tests for comments and whitespace handling within structures
}
