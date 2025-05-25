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
    CustomError(String),
    PestError(pest::error::Error<Rule>),
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

pub fn parse(input: &str) -> Result<Vec<TopLevel>, pest::error::Error<Rule>> {
    let pairs = RTFSParser::parse(Rule::program, input)?;
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

    Ok(top_level_pairs.map(build_ast).collect())
}

// --- AST Builder Functions ---

fn build_ast(pair: Pair<Rule>) -> TopLevel {
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
        | Rule::namespaced_identifier => TopLevel::Expression(build_expression(pair)),

        // Handle specific top-level definitions
        Rule::task_definition => TopLevel::Task(build_task_definition(pair).unwrap()),
        Rule::module_definition => TopLevel::Module(build_module_definition(pair).unwrap()),

        // Import definition should only appear inside a module, handle within build_module_definition
        Rule::import_definition => {
            panic!("Import definition found outside of a module context")
        }

        // Handle unexpected rules at this level
        rule => unimplemented!(
            "build_ast encountered unexpected top-level rule: {:?}, content: '{}'",
            rule,
            pair.as_str()
        ),
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
                metadata = Some(Expression::Map(build_map(value_pair)));
            }
            s if s.starts_with(":intent") => {
                intent = Some(build_expression(value_pair));
            }
            s if s.starts_with(":contracts") => {
                assert_eq!(value_pair.as_rule(), Rule::map);
                contracts = Some(Expression::Map(build_map(value_pair)));
            }
            s if s.starts_with(":plan") => {
                plan = Some(build_expression(value_pair));
            }
            s if s.starts_with(":execution-trace") => {
                assert_eq!(value_pair.as_rule(), Rule::vector);
                execution_trace = Some(Expression::Vector(
                    value_pair.into_inner().map(build_expression).collect(),
                ));
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
        .map(|p| Ok(build_symbol(p))) // build_symbol doesn't return Result, so wrap Ok
        .collect()
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
    let name = build_symbol(name_pair);

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
            Rule::WHITESPACE | Rule::COMMENT | Rule::EOI => continue, // EOI might appear if it's the last thing
            Rule::def_expr => {
                let def_node = build_def_expr(def_candidate_pair.into_inner());
                definitions.push(ModuleLevelDefinition::Def(def_node));
            }
            Rule::defn_expr => {
                eprintln!(
                    "[build_module_definition] Calling build_defn_expr for: rule={:?}, str='{}'",
                    def_candidate_pair.as_rule(),
                    def_candidate_pair.as_str()
                );
                let defn_node = build_defn_expr(def_candidate_pair.into_inner());
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
    let module_name = build_symbol(module_name_pair);

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
                        alias = Some(build_symbol(inner_pair));
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
                let mut symbols = Vec::new();
                for inner_pair in option_inner {
                    if inner_pair.as_rule() == Rule::symbol {
                        symbols.push(build_symbol(inner_pair));
                    }
                }
                if symbols.is_empty() {
                    return Err(PestParseError::CustomError(
                        "Import :only option requires at least one symbol".to_string(),
                    ));
                }
                only = Some(symbols);
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
        // Added more AST types for tests
        CatchClause,  // Added
        CatchPattern, // Added
        DefExpr,
        DefnExpr, // Added DefnExpr
        DoExpr,
        Expression,
        IfExpr,
        ImportDefinition, // Added ImportDefinition
        Keyword,
        LetBinding,
        LetExpr,
        Literal,
        LogStepExpr, // Added
        MapKey,
        // Removed unused MapMatchPattern
        MatchClause,  // Added
        MatchExpr,    // Added
        MatchPattern, // Added
        ModuleDefinition,
        ModuleLevelDefinition,
        ParallelBinding, // Added
        ParallelExpr,    // Added
        ParamDef,        // Added ParamDef
        Pattern,
        Symbol,
        TaskDefinition,
        TopLevel,
        TryCatchExpr, // Added
        TypeExpr,     // Added TypeExpr for catch/match patterns
        // Removed unused VectorMatchPattern
        WithResourceExpr, // Added
    };
    use std::collections::HashMap; // Added HashMap for map tests

    // Helper macro for asserting expression parsing
    macro_rules! assert_expr_parses_to {
        ($input:expr, $expected:expr) => {
            let parse_result = RTFSParser::parse(Rule::expression, $input);
            assert!(
                parse_result.is_ok(),
                "Failed to parse expression:\\nInput: {:?}\\nError: {:?}",
                $input,
                parse_result.err().unwrap()
            );
            // Pass the expression pair itself to build_expression
            let expr_pair = parse_result.unwrap().next().unwrap();
            let ast = expressions::build_expression(expr_pair);
            assert_eq!(
                ast, $expected,
                "Expression AST mismatch for input: {:?}\nExpected: {:#?}\nActual: {:#?}",
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
        assert_expr_parses_to!("123", Expression::Literal(Literal::Integer(123)));
        assert_expr_parses_to!("-45", Expression::Literal(Literal::Integer(-45)));
        assert_expr_parses_to!("1.23", Expression::Literal(Literal::Float(1.23)));
        assert_expr_parses_to!("-0.5", Expression::Literal(Literal::Float(-0.5)));
        assert_expr_parses_to!(
            "\"hello\"",
            Expression::Literal(Literal::String("hello".to_string()))
        );
        assert_expr_parses_to!(
            "\"hello\\\\world\\n\"", // Need double backslash for literal backslash in Rust string
            Expression::Literal(Literal::String("hello\\world\n".to_string()))
        );
        assert_expr_parses_to!("true", Expression::Literal(Literal::Boolean(true)));
        assert_expr_parses_to!("false", Expression::Literal(Literal::Boolean(false)));
        assert_expr_parses_to!("nil", Expression::Literal(Literal::Nil));
    }

    #[test]
    fn test_parse_symbol_keyword() {
        assert_expr_parses_to!("my-var", Expression::Symbol(Symbol("my-var".to_string())));
        assert_expr_parses_to!(
            "ns/my-var",
            Expression::Symbol(Symbol("ns/my-var".to_string()))
        );
        assert_expr_parses_to!(
            ":my-key",
            Expression::Keyword(Keyword("my-key".to_string()))
        );
    }

    #[test]
    fn test_parse_collections() {
        // Vector
        assert_expr_parses_to!(
            "[1 2 \"three\"]",
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
                function: Box::new(Expression::Symbol(Symbol("a".to_string()))),
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
        assert_expr_parses_to!("{ :a 1 \"b\" true }", Expression::Map(expected_map.clone()));
        assert_expr_parses_to!("{}", Expression::Map(HashMap::new()));
    }

    #[test]
    fn test_parse_if() {
        assert_expr_parses_to!(
            "(if true 1 0)",
            Expression::If(IfExpr {
                condition: Box::new(Expression::Literal(Literal::Boolean(true))),
                then_branch: Box::new(Expression::Literal(Literal::Integer(1))),
                else_branch: Some(Box::new(Expression::Literal(Literal::Integer(0)))),
            })
        );
        assert_expr_parses_to!(
            "(if x y)",
            Expression::If(IfExpr {
                condition: Box::new(Expression::Symbol(Symbol("x".to_string()))),
                then_branch: Box::new(Expression::Symbol(Symbol("y".to_string()))),
                else_branch: None,
            })
        );
    }

    #[test]
    fn test_parse_do() {
        assert_expr_parses_to!(
            "(do (a) (b))",
            Expression::Do(DoExpr {
                expressions: vec![
                    Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("a".to_string()))),
                        arguments: vec![]
                    },
                    Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("b".to_string()))),
                        arguments: vec![]
                    },
                ]
            })
        );
        assert_expr_parses_to!(
            "(do)",
            Expression::Do(DoExpr {
                expressions: vec![]
            })
        );
    }

    #[test]
    fn test_parse_def() {
        assert_expr_parses_to!(
            "(def x 1)",
            Expression::Def(DefExpr {
                symbol: Symbol("x".to_string()),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(1))),
            })
        );
        // TODO: Add test with type annotation once build_type_expr is fully tested
    }

    #[test]
    fn test_parse_let() {
        // Simple let
        assert_expr_parses_to!(
            "(let [x 1 y \"hi\"] (+ x 1))",
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
                        value: Box::new(Expression::Literal(Literal::String("hi".to_string()))),
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
        // TODO: Add tests for let with destructuring patterns once pattern parsing is fully tested
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
            "(def x 1) \n ; comment \n \"hello\"",
            vec![
                TopLevel::Expression(Expression::Def(DefExpr {
                    symbol: Symbol("x".to_string()),
                    type_annotation: None,
                    value: Box::new(Expression::Literal(Literal::Integer(1))),
                })),
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
            Expression::Keyword(Keyword("string".to_string())),
        );
        contracts_map.insert(
            MapKey::Keyword(Keyword("output".to_string())),
            Expression::Keyword(Keyword("component".to_string())),
        );
        let mut trace_map = HashMap::new();
        trace_map.insert(
            MapKey::Keyword(Keyword("step".to_string())),
            Expression::Literal(Literal::String("step-1".to_string())),
        );
        trace_map.insert(
            MapKey::Keyword(Keyword("status".to_string())),
            Expression::Keyword(Keyword("success".to_string())),
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
          (def private-val 42)
          (defn public-fn [x] ; Simple param
            (other/do-something x private-val)) ; Body expression
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
                ModuleLevelDefinition::Def(DefExpr {
                    symbol: Symbol("private-val".to_string()),
                    type_annotation: None,
                    value: Box::new(Expression::Literal(Literal::Integer(42))),
                }),
                ModuleLevelDefinition::Defn(DefnExpr {
                    name: Symbol("public-fn".to_string()),
                    params: vec![ParamDef {
                        pattern: Pattern::Symbol(Symbol("x".to_string())),
                        type_annotation: None,
                    }],
                    variadic_param: None,
                    return_type: None,
                    body: vec![Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol(
                            "other/do-something".to_string(),
                        ))),
                        arguments: vec![
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Symbol(Symbol("private-val".to_string())),
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
        println!("\n==================== AST DIFF HELP ====================");
        println!("Expected AST written to: expected_ast.txt");
        println!("Actual AST   written to: actual_ast.txt");
        println!("To see a diff, run: diff -u expected_ast.txt actual_ast.txt");
        println!("(Or use your favorite diff tool)");
        println!("======================================================\n");
        println!("Expected AST: {:#?}", expected);
        println!("Actual AST: {:#?}\n", actual);
        assert_eq!(actual, expected, "Module definition AST mismatch");
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
                resource_symbol: Symbol("res".to_string()),
                resource_type: TypeExpr::Alias(Symbol("ResourceType".to_string())), // Assuming Alias
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
        assert_expr_parses_to!(
            "(try (dangerous-op) (catch :Error e (handle e)) (catch :Other err (log err)) (finally (cleanup)))",
            Expression::TryCatch(TryCatchExpr {
                try_body: vec![
                    Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("dangerous-op".to_string()))),
                        arguments: vec![],
                    }
                ],
                catch_clauses: vec![
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("Error".to_string())),
                        binding: Symbol("e".to_string()),
                        body: vec![
                            Expression::FunctionCall {
                                function: Box::new(Expression::Symbol(Symbol("handle".to_string()))),
                                arguments: vec![Expression::Symbol(Symbol("e".to_string()))],
                            }
                        ],
                    },
                    CatchClause {
                        pattern: CatchPattern::Keyword(Keyword("Other".to_string())),
                        binding: Symbol("err".to_string()),
                        body: vec![
                            Expression::FunctionCall {
                                function: Box::new(Expression::Symbol(Symbol("log".to_string()))),
                                arguments: vec![Expression::Symbol(Symbol("err".to_string()))],
                            }
                        ],
                    },
                ],
                finally_body: Some(vec![
                    Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("cleanup".to_string()))),
                        arguments: vec![],
                    }
                ]),
            })
        );
        // Test without finally
        assert_expr_parses_to!(
            "(try (op) (catch :E x (h x)))",
            Expression::TryCatch(TryCatchExpr {
                try_body: vec![Expression::FunctionCall {
                    function: Box::new(Expression::Symbol(Symbol("op".to_string()))),
                    arguments: vec![]
                }],
                catch_clauses: vec![CatchClause {
                    pattern: CatchPattern::Keyword(Keyword("E".to_string())),
                    binding: Symbol("x".to_string()),
                    body: vec![Expression::FunctionCall {
                        function: Box::new(Expression::Symbol(Symbol("h".to_string()))),
                        arguments: vec![Expression::Symbol(Symbol("x".to_string()))]
                    }],
                }],
                finally_body: None,
            })
        );
    }

    #[test]
    fn test_parse_match() {
        // Simple literal and wildcard match
        assert_expr_parses_to!(
            "(match x (1 \"one\") (\"two\" 2) (_ 0))",
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("x".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Literal(Literal::Integer(1)),
                        guard: None,
                        body: vec![Expression::Literal(Literal::String("one".to_string()))],
                    },
                    MatchClause {
                        pattern: MatchPattern::Literal(Literal::String("two".to_string())),
                        guard: None,
                        body: vec![Expression::Literal(Literal::Integer(2))],
                    },
                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: vec![Expression::Literal(Literal::Integer(0))],
                    },
                ],
            })
        );
        // Match with guard
        assert_expr_parses_to!(
            "(match y (:a 1) (x when (> x 0) x) (_ -1))",
            Expression::Match(MatchExpr {
                expression: Box::new(Expression::Symbol(Symbol("y".to_string()))),
                clauses: vec![
                    MatchClause {
                        pattern: MatchPattern::Keyword(Keyword("a".to_string())),
                        guard: None,
                        body: vec![Expression::Literal(Literal::Integer(1))],
                    },
                    MatchClause {
                        pattern: MatchPattern::Symbol(Symbol("x".to_string())), // Symbol pattern binds
                        guard: Some(Box::new(Expression::FunctionCall {
                            function: Box::new(Expression::Symbol(Symbol(">".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("x".to_string())),
                                Expression::Literal(Literal::Integer(0))
                            ]
                        })),
                        body: vec![Expression::Symbol(Symbol("x".to_string()))],
                    },
                    MatchClause {
                        pattern: MatchPattern::Wildcard,
                        guard: None,
                        body: vec![Expression::Literal(Literal::Integer(-1))],
                    },
                ],
            })
        );
        // TODO: Add tests for vector/map/as match patterns once pattern parsing is robustly tested
    }

    #[test]
    fn test_parse_log_step() {
        let parse_result =
            RTFSParser::parse(Rule::expression, "(log-step :id \"step-1\" (do-something))");
        assert!(
            parse_result.is_ok(),
            "Failed to parse expression:\\nInput: {:?}\\nError: {:?}",
            "(log-step :id \"step-1\" (do-something))",
            parse_result.err().unwrap()
        );
        // Pass the expression pair itself to build_expression
        let expr_pair = parse_result.unwrap().next().unwrap();
        let ast = expressions::build_expression(expr_pair);
        println!("Actual AST: {:#?}", ast);

        let expected = Expression::LogStep(LogStepExpr {
            id: "step-1".to_string(),
            expression: Box::new(Expression::FunctionCall {
                function: Box::new(Expression::Symbol(Symbol("do-something".to_string()))),
                arguments: vec![],
            }),
        });
        println!("Expected AST: {:#?}", expected);

        assert_eq!(
            ast, expected,
            "Expression AST mismatch for input: {:?}",
            "(log-step :id \"step-1\" (do-something))"
        );
    }

    // TODO: Add tests for fn, defn once pattern/type parsing is robust
    // TODO: Add tests for comments and whitespace handling within structures
    // TODO: Add tests for complex patterns (map/vector destructuring in let, fn, match)
    // TODO: Add tests for complex type expressions
}
