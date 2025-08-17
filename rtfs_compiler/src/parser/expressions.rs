use super::common::{build_literal, build_map_key, build_symbol};
use super::errors::{pair_to_source_span, PestParseError};
use super::special_forms::{
    build_def_expr, build_defn_expr, build_discover_agents_expr, build_do_expr, build_fn_expr,
    build_if_expr, build_let_expr, build_log_step_expr, build_match_expr, build_parallel_expr,
    build_try_catch_expr, build_with_resource_expr,
};
use super::utils::unescape;
use super::Rule;
use crate::ast::{Expression, MapKey, Symbol}; // Symbol now used for task_context_access desugaring
use pest::iterators::Pair;
use std::collections::HashMap;

pub(super) fn build_expression(mut pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    // Drill down through silent rules like \\'expression\\' or \\'special_form\\'
    let original_pair_for_span = pair.clone(); // Clone for potential error reporting at the original level
    loop {
        let rule = pair.as_rule();
        if rule == Rule::expression || rule == Rule::special_form {
            let mut inner = pair.into_inner();
            if let Some(next) = inner.next() {
                pair = next;
            } else {
                return Err(PestParseError::InvalidInput {
                    message: "Expected inner rule for expression/special_form".to_string(),
                    span: Some(pair_to_source_span(&original_pair_for_span)),
                });
            }
        } else {
            break;
        }
    }
    let current_pair_for_span = pair.clone(); // Clone for error reporting at the current, drilled-down level
    match pair.as_rule() {
        Rule::literal => Ok(Expression::Literal(build_literal(pair)?)),
        Rule::symbol => Ok(Expression::Symbol(build_symbol(pair)?)),
        Rule::resource_ref => build_resource_ref(pair),
        // Task context access currently desugars to a plain symbol (strip leading '@' and optional ':')
        Rule::task_context_access => {
            let raw = pair.as_str(); // e.g. "@task-id" or "@:context-key"
            let without_at = &raw[1..];
            let symbol_name = if let Some(rest) = without_at.strip_prefix(':') { rest } else { without_at };
            Ok(Expression::Symbol(Symbol(symbol_name.to_string())))
        }

        Rule::vector => Ok(Expression::Vector(
            pair.into_inner()
                .map(build_expression)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Rule::map => Ok(Expression::Map(build_map(pair)?)),
        Rule::let_expr => Ok(Expression::Let(build_let_expr(pair)?)),
        Rule::if_expr => Ok(Expression::If(build_if_expr(pair)?)),
        Rule::do_expr => Ok(Expression::Do(build_do_expr(pair.into_inner())?)),
        Rule::fn_expr => Ok(Expression::Fn(build_fn_expr(pair)?)),
        Rule::def_expr => Ok(Expression::Def(Box::new(build_def_expr(pair)?))),
        Rule::defn_expr => Ok(Expression::Defn(Box::new(build_defn_expr(pair)?))),
        Rule::parallel_expr => Ok(Expression::Parallel(build_parallel_expr(pair)?)),
        Rule::with_resource_expr => Ok(Expression::WithResource(build_with_resource_expr(pair)?)),
        Rule::try_catch_expr => Ok(Expression::TryCatch(build_try_catch_expr(pair)?)),
        Rule::match_expr => Ok(Expression::Match(build_match_expr(pair)?)),
        Rule::log_step_expr => Ok(Expression::LogStep(Box::new(build_log_step_expr(pair)?))),
        Rule::discover_agents_expr => Ok(Expression::DiscoverAgents(build_discover_agents_expr(pair)?)),
        Rule::list => {
            let _list_pair_span = pair_to_source_span(&pair);
            let mut inner_pairs = pair.into_inner().peekable();

            if inner_pairs.peek().is_none() {
                // Empty list: ()
                Ok(Expression::List(vec![]))
            } else {
                // Non-empty list, potentially a function call or a data list
                let first_element_pair = inner_pairs.next().unwrap(); // We know it's not empty

                // Attempt to parse the first element.
                // We need to clone `first_element_pair` if we might need to re-parse all elements later for a data list.
                let callee_ast = build_expression(first_element_pair.clone())?;

                // Heuristic: if the first element is a Symbol, or an Fn expression,
                // or another FunctionCall, treat it as a function call.
                match callee_ast {
                    Expression::Symbol(_) | Expression::Fn(_) | Expression::FunctionCall { .. } => {
                        // It's likely a function call. Parse remaining as arguments.
                        let arguments = inner_pairs
                            .map(build_expression) // build_expression for each subsequent pair
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(Expression::FunctionCall {
                            callee: Box::new(callee_ast),
                            arguments,
                        })
                    }
                    // If the first element is not a symbol/fn/call, it's a data list.
                    _ => {
                        // Reconstruct the full list of expressions, including the first element.
                        // We already parsed `callee_ast` (the first element).
                        let mut elements = vec![callee_ast];
                        // Parse the rest of the elements.
                        for p in inner_pairs {
                            elements.push(build_expression(p)?);
                        }
                        Ok(Expression::List(elements))
                    }
                }
            }
        }
        Rule::WHEN => Err(PestParseError::InvalidInput {
            message: "'when' keyword found in unexpected context - should only appear in match expressions".to_string(),
            span: Some(pair_to_source_span(&current_pair_for_span))
        }),
        rule => Err(PestParseError::UnsupportedRule {
            rule: format!(
                "build_expression not implemented for rule: {:?} - {}",
                rule,
                current_pair_for_span.as_str()
            ),
            span: Some(pair_to_source_span(&current_pair_for_span))
        }),
    }
}

fn build_resource_ref(pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    let pair_span = pair_to_source_span(&pair);
    let mut inner = pair.into_inner();
    let _keyword_pair = inner.next(); // Skip resource_ref_keyword
    let string_pair = inner.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "Expected a string literal inside resource:ref".to_string(),
        span: Some(pair_span),
    })?; // The string literal includes the quotes, so we need to strip them and unescape.
    let raw_str = string_pair.as_str();
    let content = &raw_str[1..raw_str.len() - 1];
    let unescaped_content = unescape(content).map_err(|e| PestParseError::InvalidLiteral {
        message: format!(
            "Invalid escape sequence in resource reference string: {:?}",
            e
        ),
        span: Some(pair_to_source_span(&string_pair)),
    })?;

    Ok(Expression::ResourceRef(unescaped_content))
}



pub(super) fn build_map(pair: Pair<Rule>) -> Result<HashMap<MapKey, Expression>, PestParseError> {
    if pair.as_rule() != Rule::map {
        return Err(PestParseError::InvalidInput {
            message: format!(
                "Expected Rule::map, found {:?} for build_map",
                pair.as_rule()
            ),
            span: Some(pair_to_source_span(&pair)),
        });
    }
    // let map_span = pair_to_source_span(&pair); // This was unused
    let mut map_data = HashMap::new();
    let mut map_content = pair.into_inner();

    while let Some(entry_pair) = map_content.next() {
        if entry_pair.as_rule() == Rule::WHITESPACE || entry_pair.as_rule() == Rule::COMMENT {
            continue;
        }
        let entry_span = pair_to_source_span(&entry_pair);
        if entry_pair.as_rule() != Rule::map_entry {
            return Err(PestParseError::InvalidInput {
                message: format!(
                    "Expected map_entry inside map, found {:?}",
                    entry_pair.as_rule()
                ),
                span: Some(entry_span),
            });
        }
        let mut entry_inner = entry_pair.into_inner();
        let key_pair = entry_inner
            .next()
            .ok_or_else(|| PestParseError::InvalidInput {
                message: "Map entry missing key".to_string(),
                span: Some(entry_span.clone()),
            })?;
        let value_pair = entry_inner
            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .ok_or_else(|| PestParseError::InvalidInput {
                message: "Map entry missing value".to_string(),
                span: Some(entry_span),
            })?;
        let key = build_map_key(key_pair)?;
        let value = build_expression(value_pair)?;
        map_data.insert(key, value);
    }
    Ok(map_data)
}
