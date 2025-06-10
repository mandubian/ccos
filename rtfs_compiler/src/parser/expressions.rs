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
            }        }
        Rule::WHEN => Err(PestParseError::InvalidInput(
            "'when' keyword found in unexpected context - should only appear in match expressions".to_string()
        )),
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
