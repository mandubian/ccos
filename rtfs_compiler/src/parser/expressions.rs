use super::common::{build_keyword, build_literal, build_map_key, build_symbol}; // Import from sibling
use super::special_forms::{
    build_def_expr,
    build_defn_expr,
    build_do_expr,
    build_fn_expr,
    build_if_expr,
    build_let_expr,
    build_log_step_expr,
    build_match_expr,
    build_parallel_expr,
    build_try_catch_expr,     // Added new builders
    build_with_resource_expr, // Added new builders
};
use super::Rule; // Import Rule from the parent module (mod.rs)
use crate::ast::{Expression, MapKey};
use pest::iterators::Pair; // Removed unused Pairs
use std::collections::HashMap; // Import from sibling

pub(super) fn build_expression(mut pair: Pair<Rule>) -> Expression {
    // Drill down through silent rules like 'expression' or 'special_form'
    loop {
        let rule = pair.as_rule();
        if rule == Rule::expression || rule == Rule::special_form {
            let mut inner = pair.into_inner();
            if let Some(next) = inner.next() {
                pair = next;
            } else {
                panic!("Expected inner rule for expression/special_form");
            }
        } else {
            break;
        }
    }

    match pair.as_rule() {
        Rule::literal => Expression::Literal(build_literal(pair)),
        Rule::symbol => Expression::Symbol(build_symbol(pair)),
        Rule::keyword => Expression::Keyword(build_keyword(pair)),
        Rule::vector => Expression::Vector(pair.into_inner().map(build_expression).collect()),
        Rule::map => Expression::Map(build_map(pair)),
        Rule::let_expr => Expression::Let(build_let_expr(pair.into_inner())),
        Rule::if_expr => Expression::If(build_if_expr(pair.into_inner())),
        Rule::do_expr => {
            // Since we've improved the do_keyword rule in the grammar to properly match
            // only "do" followed by whitespace or delimiters, we can safely assume
            // that if we get here with Rule::do_expr, it's actually a "do" special form.
            Expression::Do(build_do_expr(pair.into_inner()))
        },
        Rule::fn_expr => Expression::Fn(build_fn_expr(pair.into_inner())),
        Rule::def_expr => Expression::Def(build_def_expr(pair.into_inner())),
        Rule::defn_expr => Expression::Defn(build_defn_expr(pair.into_inner())),
        Rule::parallel_expr => Expression::Parallel(build_parallel_expr(pair.into_inner())),
        Rule::with_resource_expr => {
            Expression::WithResource(build_with_resource_expr(pair.into_inner()))
        }
        Rule::try_catch_expr => Expression::TryCatch(build_try_catch_expr(pair.into_inner())),
        Rule::match_expr => Expression::Match(build_match_expr(pair.into_inner())),
        Rule::log_step_expr => Expression::LogStep(build_log_step_expr(pair.into_inner())),
        Rule::list => {
            let inner_pairs: Vec<Pair<Rule>> = pair
                .clone()
                .into_inner()
                // Filter out whitespace/comments more robustly
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                .collect();

            if inner_pairs.is_empty() {
                return Expression::List(vec![]); // Return directly
            } else {
                let first_element_pair = inner_pairs[0].clone();
                let first_element_ast = build_expression(first_element_pair.clone()); // Build AST for first element

                // --- Check for special forms disguised as lists ---
                if let Expression::Symbol(s) = &first_element_ast {
                    // Get the pairs *after* the keyword symbol
                    let mut arguments_pairs = pair.into_inner();
                    // Consume the first significant element (the keyword)
                    arguments_pairs
                        .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT);

                    // Check the EXACT symbol string
                    match s.0.as_str() {
                        // Pass the remaining pairs (arguments/body) to the builder
                        "if" => return Expression::If(build_if_expr(arguments_pairs)),
                        "do" => return Expression::Do(build_do_expr(arguments_pairs)),
                        "let" => return Expression::Let(build_let_expr(arguments_pairs)),
                        "fn" => return Expression::Fn(build_fn_expr(arguments_pairs)),
                        "def" => return Expression::Def(build_def_expr(arguments_pairs)),
                        "defn" => return Expression::Defn(build_defn_expr(arguments_pairs)),
                        "parallel" => {
                            return Expression::Parallel(build_parallel_expr(arguments_pairs))
                        }
                        "with-resource" => {
                            return Expression::WithResource(build_with_resource_expr(
                                arguments_pairs,
                            ))
                        }
                        "try" => {
                            return Expression::TryCatch(build_try_catch_expr(arguments_pairs))
                        }
                        "match" => return Expression::Match(build_match_expr(arguments_pairs)),
                        "log-step" => {
                            println!("[expressions.rs] Calling build_log_step_expr with arguments_pairs: {:?}", arguments_pairs.clone().collect::<Vec<_>>());
                            return Expression::LogStep(build_log_step_expr(arguments_pairs));
                        }
                        _ => {} // Not a special form keyword, continue below
                    }
                    // If we reached here, it was a symbol but NOT a special form keyword.
                }
                // --- End special form check ---

                // If it wasn't a special form, proceed with function call or list logic
                if matches!(first_element_ast, Expression::Symbol(_)) {
                    let args = inner_pairs // Use the filtered inner_pairs
                        .iter()
                        .skip(1) // Skip the function symbol
                        .map(|p| build_expression(p.clone()))
                        .collect();
                    // Explicitly return FunctionCall if the first element was a non-special-form symbol
                    return Expression::FunctionCall {
                        function: Box::new(first_element_ast),
                        arguments: args,
                    };
                } else {
                    // If first element wasn't a symbol, it's just a list
                    let elements = inner_pairs // Use the filtered inner_pairs
                        .iter()
                        .map(|p| build_expression(p.clone()))
                        .collect();
                    // Explicitly return List if the first element was not a symbol
                    return Expression::List(elements);
                }
            }
        }
        rule => unimplemented!(
            "build_expression not implemented for rule: {:?} - {}",
            rule,
            pair.as_str()
        ),
    }
}

// Builds a HashMap from map pairs
pub(super) fn build_map(pair: Pair<Rule>) -> HashMap<MapKey, Expression> {
    assert_eq!(pair.as_rule(), Rule::map);
    let mut map = HashMap::new();
    let mut map_content = pair.into_inner();

    while let Some(entry_pair) = map_content.next() {
        if entry_pair.as_rule() == Rule::WHITESPACE || entry_pair.as_rule() == Rule::COMMENT {
            continue;
        }

        assert_eq!(
            entry_pair.as_rule(),
            Rule::map_entry,
            "Expected map_entry inside map"
        );
        let mut entry_inner = entry_pair.into_inner();
        let key_pair = entry_inner.next().expect("Map entry missing key");
        let value_pair = entry_inner
            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .expect("Map entry missing value");

        let key = build_map_key(key_pair);
        let value = build_expression(value_pair);
        map.insert(key, value);
    }
    map
}
