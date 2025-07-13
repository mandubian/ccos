use crate::ast::{
    ImportDefinition, ModuleDefinition,
    ModuleLevelDefinition, Symbol, TopLevel,
};
use crate::parser::common::{build_symbol, next_significant};
use crate::parser::errors::{invalid_input_error, pair_to_source_span, PestParseError};
use crate::parser::expressions::build_expression;
use crate::parser::Rule;
use pest::iterators::{Pair, Pairs};

// --- AST Builder Functions ---

pub fn build_ast(pair: Pair<Rule>) -> Result<TopLevel, PestParseError> {
    let toplevel_result = match pair.as_rule() {
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
        | Rule::def_expr
        | Rule::defn_expr
        | Rule::parallel_expr
        | Rule::with_resource_expr
        | Rule::try_catch_expr
        | Rule::match_expr
        | Rule::log_step_expr
        | Rule::discover_agents_expr
        | Rule::resource_ref
        | Rule::task_context_access
        | Rule::identifier
        | Rule::namespaced_identifier => build_expression(pair).map(TopLevel::Expression),
        Rule::module_definition => build_module_definition(pair).map(TopLevel::Module),
        Rule::import_definition => Err(PestParseError::CustomError {
            message: "Import definition found outside of a module context".to_string(),
            span: Some(pair_to_source_span(&pair)),
        }),
        rule => Err(PestParseError::CustomError {
            message: format!(
                "build_ast encountered unexpected top-level rule: {:?}, content: '{}'",
                rule,
                pair.as_str()
            ),
            span: Some(pair_to_source_span(&pair)),
        }),
    };

    return toplevel_result;
}

// --- Top-Level Builders ---

fn build_export_option(
    parent_pair: &Pair<Rule>,
    mut pairs: Pairs<Rule>,
) -> Result<Vec<Symbol>, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let exports_keyword_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected :exports keyword in export_option".to_string(),
            span: Some(parent_span.clone()),
        })?;
    if exports_keyword_pair.as_rule() != Rule::exports_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: ":exports keyword".to_string(),
            found: format!("{:?}", exports_keyword_pair.as_rule()),
            rule_text: exports_keyword_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair)),
        });
    }

    let symbols_vec_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected symbols vector in export_option".to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair).end_as_start()),
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
        .map(|p| build_symbol(p.clone()))
        .collect::<Result<Vec<Symbol>, PestParseError>>()
}

fn build_module_definition(pair: Pair<Rule>) -> Result<ModuleDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "module" keyword
    let name_pair = next_significant(&mut inner)
        .ok_or_else(|| invalid_input_error("Missing module name", &pair))?;
    let name = build_symbol(name_pair)?;

    // Parse exports if present
    let mut exports = None;
    if let Some(next_pair) = inner.peek() {
        if next_pair.as_rule() == Rule::export_option {
            let export_pair = inner.next().unwrap();
            let export_symbols =
                build_export_option(&export_pair, export_pair.clone().into_inner())?;
            exports = Some(export_symbols);
        }
    }

    // Parse definitions
    let mut definitions = Vec::new();
    for def_pair in inner {
        match def_pair.as_rule() {
            Rule::def_expr => {
                let def_expr = crate::parser::special_forms::build_def_expr(def_pair)?;
                definitions.push(ModuleLevelDefinition::Def(def_expr));
            }
            Rule::defn_expr => {
                let defn_expr = crate::parser::special_forms::build_defn_expr(def_pair)?;
                definitions.push(ModuleLevelDefinition::Defn(defn_expr));
            }
            Rule::import_definition => {
                // For now, skip import definitions as they're not fully implemented
                // TODO: Implement import definition parsing
                continue;
            }
            _ => {
                // Skip whitespace and other non-definition rules
                continue;
            }
        }
    }

    Ok(ModuleDefinition {
        name,
        docstring: None, // TODO: Parse docstring if present
        exports,
        definitions,
    })
}

// import_definition =  { "(" ~ import_keyword ~ namespaced_identifier ~ import_options? ~ ")" }
// fn build_import_definition(pair: Pair<Rule>) -> ImportDefinition { // Old signature
fn build_import_definition(
    parent_pair: &Pair<Rule>,
    mut pairs: Pairs<Rule>,
) -> Result<ImportDefinition, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let import_keyword_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected :import keyword in import_definition".to_string(),
            span: Some(parent_span.clone()),
        })?;
    // ... implementation needed
    Err(PestParseError::UnsupportedRule {
        rule: "import_definition".to_string(),
        span: Some(parent_span),
    })
}
