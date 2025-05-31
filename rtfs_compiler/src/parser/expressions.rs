use super::common::{build_literal, build_map_key, build_symbol}; // Removed build_keyword
use super::special_forms::{
    build_def_expr, build_defn_expr, build_do_expr, build_fn_expr, build_if_expr, build_let_expr,
    build_log_step_expr, build_match_expr, build_parallel_expr, build_try_catch_expr,
    build_with_resource_expr,
};
use super::{PestParseError, Rule}; // Added PestParseError
use crate::ast::{Expression, MapKey};
use pest::iterators::Pair;
use std::collections::HashMap;

pub(super) fn build_expression(mut pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    // Drill down through silent rules like \\\'expression\\\' or \\\'special_form\\\'
    loop {
        let rule = pair.as_rule();
        if rule == Rule::expression || rule == Rule::special_form {
            let mut inner = pair.into_inner();
            if let Some(next) = inner.next() {
                pair = next;
            } else {
                return Err(PestParseError::InvalidInput(
                    "Expected inner rule for expression/special_form".to_string(),
                ));
            }
        } else {
            break;
        }
    }

    match pair.as_rule() {
        Rule::literal => Ok(Expression::Literal(build_literal(pair)?)),
        Rule::symbol => Ok(Expression::Symbol(build_symbol(pair)?)),
        Rule::vector => Ok(Expression::Vector(
            pair.into_inner()
                .map(build_expression)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Rule::map => Ok(Expression::Map(build_map(pair)?)),
        Rule::let_expr => Ok(Expression::Let(build_let_expr(pair.into_inner())?)),
        Rule::if_expr => Ok(Expression::If(build_if_expr(pair.into_inner())?)),
        Rule::do_expr => Ok(Expression::Do(build_do_expr(pair.into_inner())?)),
        Rule::fn_expr => Ok(Expression::Fn(build_fn_expr(pair.into_inner())?)),
        Rule::def_expr => Ok(Expression::Def(Box::new(build_def_expr(
            pair.into_inner(),
        )?))),
        Rule::defn_expr => Ok(Expression::Defn(Box::new(build_defn_expr(
            pair.into_inner(),
        )?))),
        Rule::parallel_expr => Ok(Expression::Parallel(build_parallel_expr(
            pair.into_inner(),
        )?)),
        Rule::with_resource_expr => Ok(Expression::WithResource(build_with_resource_expr(
            pair.into_inner(),
        )?)),
        Rule::try_catch_expr => Ok(Expression::TryCatch(build_try_catch_expr(
            pair.into_inner(),
        )?)),
        Rule::match_expr => Ok(Expression::Match(Box::new(build_match_expr(
            pair.into_inner(),
        )?))),
        Rule::log_step_expr => Ok(Expression::LogStep(Box::new(build_log_step_expr(
            pair.into_inner(),
        )?))),
        Rule::list => {
            let inner_pairs: Vec<Pair<Rule>> = pair
                .clone()
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                .collect();

            if inner_pairs.is_empty() {
                return Ok(Expression::List(vec![]));
            } else {
                let first_element_pair = inner_pairs[0].clone();
                let first_element_ast = build_expression(first_element_pair.clone())?;

                if let Expression::Symbol(s) = &first_element_ast {
                    let mut arguments_pairs = pair.into_inner();
                    // Consume the first element (the symbol itself) from arguments_pairs
                    arguments_pairs
                        .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT);

                    match s.0.as_str() {
                        "if" => return Ok(Expression::If(build_if_expr(arguments_pairs)?)),
                        "do" => return Ok(Expression::Do(build_do_expr(arguments_pairs)?)),
                        "let" => return Ok(Expression::Let(build_let_expr(arguments_pairs)?)),
                        "fn" => return Ok(Expression::Fn(build_fn_expr(arguments_pairs)?)),
                        "def" => {
                            return Ok(Expression::Def(Box::new(build_def_expr(arguments_pairs)?)))
                        }
                        "defn" => {
                            return Ok(Expression::Defn(Box::new(build_defn_expr(
                                arguments_pairs,
                            )?)))
                        }
                        "parallel" => {
                            return Ok(Expression::Parallel(build_parallel_expr(arguments_pairs)?))
                        }
                        "with-resource" => {
                            return Ok(Expression::WithResource(build_with_resource_expr(
                                arguments_pairs,
                            )?))
                        }
                        "try" => {
                            return Ok(Expression::TryCatch(build_try_catch_expr(arguments_pairs)?))
                        }
                        "match" => {
                            return Ok(Expression::Match(Box::new(build_match_expr(
                                arguments_pairs,
                            )?)))
                        }
                        "log-step" => {
                            return Ok(Expression::LogStep(Box::new(build_log_step_expr(
                                arguments_pairs,
                            )?)));
                        }
                        _ => {} // Fall through to general function call or list
                    }
                }

                // Re-evaluate if it's a function call after potential special form handling
                if matches!(first_element_ast, Expression::Symbol(_))
                    || matches!(first_element_ast, Expression::Fn(_)) // Allow ( (fn [] ...) args )
                    || matches!(first_element_ast, Expression::FunctionCall { .. })
                // Allow ( (another-call) args)
                {
                    let args = inner_pairs
                        .iter()
                        .skip(1)
                        .map(|p| build_expression(p.clone()))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Expression::FunctionCall {
                        function: Box::new(first_element_ast),
                        arguments: args,
                    });
                } else {
                    // If the first element is not a symbol, it's a list of expressions
                    // We need to re-collect all elements including the first one
                    let elements = inner_pairs
                        .iter()
                        .map(|p| build_expression(p.clone()))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Expression::List(elements));
                }
            }
        }
        rule => Err(PestParseError::UnsupportedRule(format!(
            "build_expression not implemented for rule: {:?} - {}",
            rule,
            pair.as_str()
        ))),
    }
}

pub(super) fn build_map(pair: Pair<Rule>) -> Result<HashMap<MapKey, Expression>, PestParseError> {
    if pair.as_rule() != Rule::map {
        return Err(PestParseError::InvalidInput(format!(
            "Expected Rule::map, found {:?} for build_map",
            pair.as_rule()
        )));
    }
    let mut map = HashMap::new();
    let mut map_content = pair.into_inner();

    while let Some(entry_pair) = map_content.next() {
        if entry_pair.as_rule() == Rule::WHITESPACE || entry_pair.as_rule() == Rule::COMMENT {
            continue;
        }

        if entry_pair.as_rule() != Rule::map_entry {
            return Err(PestParseError::InvalidInput(format!(
                "Expected map_entry inside map, found {:?}",
                entry_pair.as_rule()
            )));
        }
        let mut entry_inner = entry_pair.into_inner();
        let key_pair = entry_inner
            .next()
            .ok_or_else(|| PestParseError::InvalidInput("Map entry missing key".to_string()))?;
        let value_pair = entry_inner
            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .ok_or_else(|| PestParseError::InvalidInput("Map entry missing value".to_string()))?;

        let key = build_map_key(key_pair)?;
        let value = build_expression(value_pair)?;
        map.insert(key, value);
    }
    Ok(map)
}
