use crate::ast::{
    ActionDefinition, CapabilityDefinition, ImportDefinition, IntentDefinition, ModuleDefinition,
    PlanDefinition, Property, ResourceDefinition, Symbol, TopLevel,
};
use crate::parser::common::{build_keyword, build_symbol, next_significant};
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
        | Rule::letrec_expr
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
        Rule::object_definition => {
            // object_definition contains intent_definition | plan_definition | etc.
            let inner_pair =
                pair.clone()
                    .into_inner()
                    .next()
                    .ok_or_else(|| PestParseError::CustomError {
                        message: "object_definition should contain one object type".to_string(),
                        span: Some(pair_to_source_span(&pair)),
                    })?;
            build_ast(inner_pair)
        }
        Rule::intent_definition => build_intent_definition(pair).map(TopLevel::Intent),
        Rule::plan_definition => build_plan_definition(pair).map(TopLevel::Plan),
        Rule::action_definition => build_action_definition(pair).map(TopLevel::Action),
        Rule::capability_definition => build_capability_definition(pair).map(TopLevel::Capability),
        Rule::resource_definition => build_resource_definition(pair).map(TopLevel::Resource),
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

fn build_property(pair: Pair<Rule>) -> Result<Property, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let key_pair = next_significant(&mut inner)
        .ok_or_else(|| invalid_input_error("Missing keyword in property", &pair))?;
    let value_pair = next_significant(&mut inner)
        .ok_or_else(|| invalid_input_error("Missing expression in property", &key_pair))?;

    let key = build_keyword(key_pair)?;
    let value = build_expression(value_pair)?;

    Ok(Property { key, value })
}

fn build_core_object_properties(
    pair: &Pair<Rule>,
    mut inner: Pairs<Rule>,
) -> Result<(Symbol, Vec<Property>), PestParseError> {
    let name_pair = next_significant(&mut inner).ok_or_else(|| {
        invalid_input_error(
            "Missing name/versioned_type for core object definition",
            pair,
        )
    })?;
    let name = build_symbol(name_pair)?;

    let properties = inner
        .filter(|p| p.as_rule() == Rule::property)
        .map(build_property)
        .collect::<Result<Vec<_>, _>>()?;

    Ok((name, properties))
}

fn build_intent_definition(pair: Pair<Rule>) -> Result<IntentDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "intent" keyword
    let (name, properties) = build_core_object_properties(&pair, inner)?;
    Ok(IntentDefinition { name, properties })
}

fn build_plan_definition(pair: Pair<Rule>) -> Result<PlanDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "plan" keyword
    let (name, properties) = build_core_object_properties(&pair, inner)?;
    Ok(PlanDefinition { name, properties })
}

fn build_action_definition(pair: Pair<Rule>) -> Result<ActionDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "action" keyword
    let (name, properties) = build_core_object_properties(&pair, inner)?;
    Ok(ActionDefinition { name, properties })
}

fn build_capability_definition(pair: Pair<Rule>) -> Result<CapabilityDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "capability" keyword
    let (name, properties) = build_core_object_properties(&pair, inner)?;
    Ok(CapabilityDefinition { name, properties })
}

fn build_resource_definition(pair: Pair<Rule>) -> Result<ResourceDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "resource" keyword
    let (name, properties) = build_core_object_properties(&pair, inner)?;
    Ok(ResourceDefinition { name, properties })
}

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

    // For now, create a minimal module definition
    // TODO: Parse exports, docstring, and definitions
    Ok(ModuleDefinition {
        name,
        docstring: None,
        exports: None,
        definitions: vec![],
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
