use super::Rule;
use pest::iterators::{Pair, Pairs};

// AST Node Imports - Ensure all used AST nodes are listed here
use crate::ast::{
    CatchClause, CatchPattern, DefExpr, DefnExpr, DoExpr, Expression, FnExpr, IfExpr, LetBinding, LetExpr, LogStepExpr, MapMatchPattern, MatchClause, MatchExpr, MatchPattern, ParallelBinding, ParallelExpr, ParamDef, Pattern, Symbol, TryCatchExpr, TypeExpr, VectorMatchPattern, WithResourceExpr
    // Add other AST nodes like Literal, Keyword if they are directly constructed here,
    // though often they come from build_literal, build_keyword etc.
};
use crate::parser::next_significant;

// Builder function imports from sibling modules
use super::common::{build_keyword, build_literal, build_map_key, build_pattern, build_symbol};
use super::expressions::build_expression;
use super::types::build_type_expr; // For type annotations

// Utility imports (if any) - e.g., for skipping whitespace/comments if not handled by Pest rules
// use crate::parser::utils::next_significant_pair; // Example if you have such a utility
use super::utils::unescape; // For log_step_expr

pub(super) fn build_let_expr(pairs: Pairs<Rule>) -> LetExpr {
    let mut significant_pairs = pairs.peekable();

    // 1. Consume let_keyword if present (handles calls from `Rule::let_expr` and list form)
    while let Some(p) = significant_pairs.peek() {
        match p.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => {
                significant_pairs.next(); // Consume
            }
            Rule::let_keyword => {
                significant_pairs.next(); // Consume let_keyword
                // Consume any whitespace/comment immediately after let_keyword
                while let Some(sp) = significant_pairs.peek() {
                    if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                        significant_pairs.next();
                    } else {
                        break;
                    }
                }
                break; // Found and consumed let_keyword, proceed to bindings
            }
            _ => break, // Found something else, assume it's the bindings or start of body
        }
    }

    // 2. Get the bindings pair. Expecting Rule::vector based on common syntax and test case.
    let bindings_pair_outer = significant_pairs
        .next()
        .expect("Let expression requires a binding vector");

    let mut bindings = Vec::new();
    match bindings_pair_outer.as_rule() {
        Rule::vector => {
            let mut binding_content_pairs = bindings_pair_outer
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT);

            while let Some(pattern_candidate_pair) = binding_content_pairs.next() {
                let pattern = build_pattern(pattern_candidate_pair); // build_pattern from common.rs

                // TODO: Handle optional type annotations for patterns here if grammar supports it
                // e.g., [x :Type 1 ...]. This would require peeking for ':' or type_expr.
                // For now, assuming no type annotations directly in the [pat val pat val] form.
                let type_annotation: Option<TypeExpr> = None;

                let value_pair = binding_content_pairs.next().expect(
                    "Let binding vector expects pairs of pattern and value expressions (missing value)",
                );
                let value = build_expression(value_pair);
                bindings.push(LetBinding {
                    pattern,
                    type_annotation,
                    value: Box::new(value),
                });
            }
        }
        // TODO: Consider if Rule::binding_vector (a more structured form) is also supported.
        // If so, add another arm here to parse it.
        // Rule::binding_vector => { ... }
        actual_rule => {
            panic!(
                "Expected Rule::vector for let bindings, found {:?} ('{}')",
                actual_rule,
                bindings_pair_outer.as_str()
            );
        }
    }

    // 3. The rest of significant_pairs are body expressions
    let body = significant_pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect();

    LetExpr { bindings, body }
}

pub(super) fn build_if_expr(mut pairs: Pairs<Rule>) -> IfExpr {
    // if_expr = { "(" ~ if_kw ~ expression ~ expression ~ expression? ~ ")" }
    // pairs = condition, then_branch, else_branch?
    let condition = Box::new(build_expression(pairs.next().unwrap()));
    let then_branch = Box::new(build_expression(pairs.next().unwrap()));
    let else_branch = pairs.next().map(|p| Box::new(build_expression(p))); // Optional else

    IfExpr {
        condition,
        then_branch,
        else_branch,
    }
}

pub(super) fn build_do_expr(pairs: Pairs<Rule>) -> DoExpr {
    // When called from `Rule::do_expr` in expressions.rs, 
    // `pairs` will be from `do_expr.into_inner()`.
    // Grammar: `do_expr = { "(" ~ do_keyword ~ expression* ~ ")" }`
    // So, the inner pairs will start with `do_keyword` followed by `expression*`.

    // When called from a list `("do" ...)` in `build_expression` (expressions.rs),
    // `pairs` will be `arguments_pairs` which are already the `expression*` (the "do" symbol is consumed by the caller).

    let mut significant_pairs = pairs.peekable();

    // Advance past any initial whitespace or comments
    while let Some(p) = significant_pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            significant_pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `do_keyword`, consume it.
    // This handles the case where `build_do_expr` is called from the `Rule::do_expr` match arm.
    if let Some(first_token) = significant_pairs.peek() {
        if first_token.as_rule() == Rule::do_keyword {
            significant_pairs.next(); // Consume the do_keyword
        }
    }
    
    // Now, map over the actual expressions, filtering any further non-significant tokens
    let expressions = significant_pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect();
    DoExpr { expressions }
}

pub(super) fn build_fn_expr(mut pairs: Pairs<Rule>) -> FnExpr {
    // fn_expr = { "(" ~ fn_keyword ~ fn_param_list ~ (":" ~ type_expr)? ~ expression+ ~ ")" }
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `fn_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::fn_keyword {
            pairs.next(); // Consume the fn_keyword
        }
    }

    // 1. Parameters
    let params_pair = pairs.next().expect("fn requires parameters list");
    // Expect Rule::fn_param_list based on updated grammar
    assert_eq!(
        params_pair.as_rule(),
        Rule::fn_param_list, // Changed from Rule::vector
        "Expected fn_param_list for fn params"
    );

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param = None; // Changed name to match AST
    let mut params_inner = params_pair.into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        // Skip whitespace/comments
        if param_item_peek.as_rule() == Rule::COMMENT
            || param_item_peek.as_rule() == Rule::WHITESPACE
        {
            params_inner.next(); // Consume whitespace/comment
            continue;
        }

        // Check for variadic '&'
        if param_item_peek.as_str() == "&" {
            params_inner.next(); // Consume '&'
                                 // Consume potential whitespace after &
            while let Some(peeked) = params_inner.peek() {
                if peeked.as_rule() == Rule::COMMENT || peeked.as_rule() == Rule::WHITESPACE {
                    params_inner.next(); // Consume
                } else {
                    break;
                }
            }
            let rest_symbol_pair = params_inner.next().expect("& must be followed by a symbol");
            assert_eq!(
                rest_symbol_pair.as_rule(),
                Rule::symbol,
                "& must be followed by a symbol"
            );
            variadic_param = Some(build_symbol(rest_symbol_pair));
            break; // No more regular params after &
        }

        // Process regular parameter (pattern + optional type)
        let pattern_pair = params_inner.next().unwrap(); // Consume the pattern part
        assert!(
            pattern_pair.as_rule() == Rule::binding_pattern
                || pattern_pair.as_rule() == Rule::param_def,
            "Expected binding_pattern or param_def in fn params"
        );

        // If it's param_def, it might contain the type internally
        let (pattern, type_annotation) = if pattern_pair.as_rule() == Rule::param_def {
            let mut inner_param_def = pattern_pair.into_inner();
            let pattern = build_pattern(inner_param_def.next().unwrap());
            let type_ann = inner_param_def
                .find(|p| p.as_rule() == Rule::type_expr)
                .map(build_type_expr);
            (pattern, type_ann)
        } else {
            // It's just a binding_pattern, check *next* item for type
            let pattern = build_pattern(pattern_pair);
            let mut type_ann: Option<TypeExpr> = None;
            // Peek for optional type annotation, consuming whitespace
            while let Some(peeked) = params_inner.peek() {
                if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                    params_inner.next(); // Consume whitespace/comment
                } else if peeked.as_rule() == Rule::type_expr {
                    // Found type annotation, consume it
                    type_ann = Some(build_type_expr(params_inner.next().unwrap()));
                    break; // Stop peeking for type
                } else {
                    break; // Next token is not type or whitespace
                }
            }
            (pattern, type_ann)
        };

        params.push(ParamDef {
            pattern,
            type_annotation,
        });
    }

    // 2. Body (Optional Return Type + Expressions)
    let mut body_pairs = pairs.peekable();
    let mut return_type = None;

    // Consume potential whitespace/comments before body
    while let Some(peeked) = body_pairs.peek() {
        if peeked.as_rule() == Rule::COMMENT || peeked.as_rule() == Rule::WHITESPACE {
            body_pairs.next(); // Consume
        } else {
            break;
        }
    }

    // Check if the first body element is a type expression (return type)
    if let Some(first_body_pair) = body_pairs.peek() {
        if first_body_pair.as_rule() == Rule::type_expr {
            return_type = Some(build_type_expr(body_pairs.next().unwrap()));
            // Consume potential whitespace/comments after return type
            while let Some(peeked) = body_pairs.peek() {
                if peeked.as_rule() == Rule::COMMENT || peeked.as_rule() == Rule::WHITESPACE {
                    body_pairs.next(); // Consume
                } else {
                    break;
                }
            }
        }
    }

    // The rest are body expressions
    let body = body_pairs.map(build_expression).collect();

    FnExpr {
        params,
        variadic_param,
        return_type,
        body,
    } // Updated field name
}

pub(super) fn build_def_expr(mut pairs: Pairs<Rule>) -> DefExpr {
    // def_expr = { "(" ~ def_keyword ~ symbol ~ (":" ~ type_expr)? ~ expression ~ ")" }
    // When called from `Rule::def_expr` in expressions.rs, 
    // `pairs` will be from `def_expr.into_inner()`.
    // So, the inner pairs will start with `def_keyword` followed by `symbol`, `type_expr?`, `expression`.

    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `def_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::def_keyword {
            pairs.next(); // Consume the def_keyword
        }
    }

    // Now get the symbol
    let symbol_pair = pairs.next().unwrap();
    assert_eq!(symbol_pair.as_rule(), Rule::symbol);
    let symbol = build_symbol(symbol_pair);

    let mut type_annotation = None;
    let mut value_pair = pairs.next().unwrap(); // Assume it's value initially

    // Check if the first element after name is a type_expr
    if value_pair.as_rule() == Rule::type_expr {
        type_annotation = Some(build_type_expr(value_pair));
        // Consume potential whitespace/comments after type
        while let Some(peeked) = pairs.peek() {
            if peeked.as_rule() == Rule::COMMENT || peeked.as_rule() == Rule::WHITESPACE {
                pairs.next(); // Consume
            } else {
                break;
            }
        }
        value_pair = pairs
            .next()
            .expect("def requires a value expression after type annotation");
    }

    let value = Box::new(build_expression(value_pair));

    DefExpr {
        symbol,
        type_annotation,
        value,
    } // Updated field name
}

pub(super) fn build_defn_expr(mut pairs: Pairs<Rule>) -> DefnExpr {
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `defn_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::defn_keyword {
            pairs.next(); // Consume the defn_keyword
        }
    }

    // Now get the symbol for the function name
    let name = build_symbol(pairs.next().expect("defn_expr: missing function name symbol"));

    // 3. Consume fn_param_list for parameters
    let params_list_pair = pairs.next().expect("defn_expr: missing parameters list (fn_param_list)");

    assert_eq!(
        params_list_pair.as_rule(),
        Rule::fn_param_list,
        "defn_expr: Expected fn_param_list for params, found {:?} ('{}')",
        params_list_pair.as_rule(),
        params_list_pair.as_str()
    );

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param = None;
    let mut params_inner = params_list_pair.into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        // Skip whitespace/comments
        if param_item_peek.as_rule() == Rule::COMMENT || param_item_peek.as_rule() == Rule::WHITESPACE {
            params_inner.next(); // Consume whitespace/comment
            continue;
        }

        // Check for variadic '&'
        if param_item_peek.as_str() == "&" {
            params_inner.next(); // Consume '&'
            // Consume potential whitespace after &
            while let Some(peeked) = params_inner.peek() {
                if peeked.as_rule() == Rule::COMMENT || peeked.as_rule() == Rule::WHITESPACE {
                    params_inner.next(); // Consume
                } else {
                    break;
                }
            }
            let rest_symbol_pair = params_inner.next().expect("& must be followed by a symbol");
            assert_eq!(
                rest_symbol_pair.as_rule(),
                Rule::symbol,
                "& must be followed by a symbol"
            );
            variadic_param = Some(build_symbol(rest_symbol_pair));
            break; // No more regular params after &
        }

        // Process regular parameter (param_def)
        // fn_param_list = { "[" ~ param_def* ~ ("&" ~ symbol)? ~ "]" }
        // param_def = { binding_pattern ~ (":" ~ type_expr)? }
        let param_def_pair = params_inner.next().expect("defn_expr: expected param_def in parameter list");
        assert_eq!(
            param_def_pair.as_rule(),
            Rule::param_def,
            "defn_expr: Expected param_def in fn_param_list, found {:?} ('{}')",
            param_def_pair.as_rule(), param_def_pair.as_str()
        );
        
        let mut inner_param_def = param_def_pair.into_inner();
        let pattern_pair = inner_param_def.next().expect("defn_expr: param_def requires a pattern");
        // pattern_pair should be Rule::binding_pattern based on param_def grammar
        // build_pattern handles various pattern types including direct symbols if binding_pattern is silent
        let pattern = build_pattern(pattern_pair); 
        
        let type_annotation = inner_param_def
            .find(|p| p.as_rule() == Rule::type_expr)
            .map(build_type_expr);

        params.push(ParamDef { pattern, type_annotation });
    }

    // 4. Optional return type (from the remaining pairs of defn_expr)
    let mut return_type = None;
    // Peek at the next item in the main `pairs` iterator (children of defn_expr)
    if let Some(peeked_pair) = pairs.peek() {
        if peeked_pair.as_rule() == Rule::type_expr {
            return_type = Some(build_type_expr(pairs.next().unwrap()));
        }
    }

    // 5. Body expressions (one or more, remaining items in `pairs`)
    let body_pairs_for_debug: Vec<_> = pairs.peekable().collect(); // Collect for debug printing
    for (i, p) in body_pairs_for_debug.iter().enumerate() {
        eprintln!("[build_defn_expr] Body pair {}: rule={:?}, str='{}'", i, p.as_rule(), p.as_str());
    }
    // Re-create the iterator for actual use
    let pairs = body_pairs_for_debug.into_iter().peekable();

    let body: Vec<Expression> = pairs.map(build_expression).collect();
    if body.is_empty() {
        panic!("defn_expr: Body cannot be empty. Function: '{}'", name.0);
    }

    DefnExpr {
        name,
        params,
        variadic_param,
        return_type,
        body,
    }
}

// --- New Special Form Builders ---

// parallel_expr = { "(" ~ parallel_keyword ~ parallel_binding+ ~ ")" }
pub(super) fn build_parallel_expr(mut pairs: Pairs<Rule>) -> ParallelExpr {
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `parallel_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::parallel_keyword {
            pairs.next(); // Consume the parallel_keyword
        }
    }

    let bindings: Vec<_> = pairs.map(build_parallel_binding).collect();
    ParallelExpr { bindings }
}

// parallel_binding = { "[" ~ symbol ~ (":" ~ type_expr)? ~ expression ~ "]" }
fn build_parallel_binding(pair: Pair<Rule>) -> ParallelBinding {
    assert_eq!(pair.as_rule(), Rule::parallel_binding);
    println!("Building parallel binding from: '{}'", pair.as_str());
    let mut inner = pair.into_inner();

    // Debug all inner pairs
    let all_inner: Vec<_> = inner.clone().collect();
    println!("  Inner pairs:");
    for (i, p) in all_inner.iter().enumerate() {
        println!("    {}: rule={:?}, text='{}'", i, p.as_rule(), p.as_str());
    }

    let symbol = build_symbol(next_significant(&mut inner).expect("Parallel binding needs symbol"));
    println!("  Built symbol: {:?}", symbol);

    let mut type_annotation = None;
    let mut next_pair = next_significant(&mut inner).expect("Parallel binding needs expression");
    println!("  Next pair after symbol: rule={:?}, text='{}'", next_pair.as_rule(), next_pair.as_str());

    // Check if we have a type annotation
    if next_pair.as_rule() == Rule::type_annotation {
        // Parse the type annotation - it contains ": type_expr"
        let mut type_inner = next_pair.into_inner();
        // Skip the ":" and get the type_expr
        let type_expr_pair = type_inner.find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .expect("type_annotation should contain type_expr");
        println!("  Found type annotation, type_expr: rule={:?}, text='{}'", type_expr_pair.as_rule(), type_expr_pair.as_str());
        
        type_annotation = Some(build_type_expr(type_expr_pair));
        println!("  Built type annotation: {:?}", type_annotation);
        
        next_pair = next_significant(&mut inner).expect("Parallel binding needs expression after type");
        println!("  Next pair after type: rule={:?}, text='{}'", next_pair.as_rule(), next_pair.as_str());
    }

    let expression = Box::new(build_expression(next_pair));
    println!("  Built expression: {:?}", expression);

    let result = ParallelBinding {
        symbol,
        type_annotation,
        expression,
    };
    println!("  Final parallel binding: {:?}", result);
    result
}

// with_resource_expr = { "(" ~ with_resource_keyword ~ "[" ~ symbol ~ type_expr ~ expression ~ "]" ~ expression+ ~ ")" }
pub(super) fn build_with_resource_expr(mut pairs: Pairs<Rule>) -> WithResourceExpr {
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `with_resource_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::with_resource_keyword {
            pairs.next(); // Consume the with_resource_keyword
        }
    }

    // The next three pairs are the binding: symbol, type_expr, expression
    let resource_symbol = build_symbol(next_significant(&mut pairs).expect("with-resource needs symbol"));
    let resource_type = build_type_expr(next_significant(&mut pairs).expect("with-resource needs type"));
    let resource_init = Box::new(build_expression(
        next_significant(&mut pairs).expect("with-resource needs init expression"),
    ));

    let body = pairs.map(build_expression).collect();

    WithResourceExpr {
        resource_symbol,
        resource_type,
        resource_init,
        body,
    }
}

// try_catch_expr = { "(" ~ try_keyword ~ expression+ ~ catch_clause* ~ finally_clause? ~ ")" }
pub(super) fn build_try_catch_expr(pairs: Pairs<Rule>) -> TryCatchExpr {
    let mut try_body = Vec::new();
    let mut catch_clauses = Vec::new();
    let mut finally_body: Option<Vec<crate::ast::Expression>> = None; // Use full path for clarity
    let mut current_section = "try"; // "try", "catch", "finally"

    // Filter out the pairs to handle try_keyword if present
    let filtered_pairs: Vec<Pair<Rule>> = pairs
        .filter(|p| match p.as_rule() {
            Rule::try_keyword => false, // Skip try_keyword
            Rule::WHITESPACE | Rule::COMMENT => false, // Skip whitespace and comments
            _ => true,
        })
        .collect();

    for pair in filtered_pairs {
        match pair.as_rule() {
            Rule::expression => match current_section {
                "try" => try_body.push(build_expression(pair)),
                "finally" => {
                    // Explicitly type the temporary body vector
                    let mut body: Vec<crate::ast::Expression> =
                        finally_body.take().unwrap_or_default();
                    body.push(build_expression(pair));
                    finally_body = Some(body);
                }
                _ => panic!("Unexpected expression in try/catch/finally"),
            },
            Rule::catch_clause => {
                current_section = "catch";
                catch_clauses.push(build_catch_clause(pair));
            }
            Rule::finally_clause => {
                current_section = "finally";
                // The expressions are inside the finally_clause rule
                finally_body = Some(
                    pair.into_inner()
                        .filter(|p| p.as_rule() == Rule::expression)
                        .map(build_expression)
                        .collect(),
                );
            }
            rule => panic!("Unexpected rule in try_catch_expr: {:?}", rule),
        }
    }

    TryCatchExpr {
        try_body,
        catch_clauses,
        finally_body,
    }
}

// catch_clause = { "(" ~ "catch" ~ catch_pattern ~ symbol ~ expression+ ~ ")" }
fn build_catch_clause(pair: Pair<Rule>) -> CatchClause {
    assert_eq!(pair.as_rule(), Rule::catch_clause);
    let mut inner = pair.into_inner();

    // Skip "catch" keyword if present as a token
    let pattern_pair = next_significant(&mut inner).expect("Catch clause needs pattern");
    let pattern = build_catch_pattern(pattern_pair);

    let binding =
        build_symbol(next_significant(&mut inner).expect("Catch clause needs binding symbol"));

    let body = inner
        .filter(|p| p.as_rule() == Rule::expression)
        .map(build_expression)
        .collect();

    CatchClause {
        pattern,
        binding,
        body,
    }
}

// catch_pattern = _{ type_expr | keyword | symbol }
fn build_catch_pattern(pair: Pair<Rule>) -> CatchPattern {
    // Drill down if needed
    let actual_pair = match pair.as_rule() {
        Rule::catch_pattern => pair.into_inner().next().unwrap(),
        _ => pair,
    };
    match actual_pair.as_rule() {
        Rule::type_expr => CatchPattern::Type(build_type_expr(actual_pair)),
        Rule::keyword => CatchPattern::Keyword(build_keyword(actual_pair)),
        Rule::symbol => CatchPattern::Symbol(build_symbol(actual_pair)),
        rule => unimplemented!("build_catch_pattern not implemented for rule: {:?}", rule),
    }
}

// match_expr = { "(" ~ match_keyword ~ expression ~ match_clause+ ~ ")" }
pub(super) fn build_match_expr(mut pairs: Pairs<Rule>) -> MatchExpr {
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `match_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::match_keyword {
            pairs.next(); // Consume the match_keyword
        }
    }

    let expression = Box::new(build_expression(
        pairs.next().expect("Match expression needs value to match"),
    ));
    let clauses = pairs.map(build_match_clause).collect();
    MatchExpr {
        expression,
        clauses,
    }
}

// match_clause = { "(" ~ match_pattern ~ ("when" ~ expression)? ~ expression+ ~ ")" }
fn build_match_clause(pair: Pair<Rule>) -> MatchClause {
    println!("Match clause raw: '{}'", pair.as_str());
    assert_eq!(pair.as_rule(), Rule::match_clause);
    
    // Let's debug all inner pairs first
    let inner_debug = pair.clone().into_inner()
        .map(|p| format!("Rule: {:?}, Text: '{}'", p.as_rule(), p.as_str()))
        .collect::<Vec<_>>();
    println!("Match clause inner pairs: {:#?}", inner_debug);
    
    let mut inner = pair.into_inner();

    let pattern_pair = next_significant(&mut inner).expect("Match clause needs pattern");
    println!("Pattern pair: {:?} - \'{}\'", pattern_pair.as_rule(), pattern_pair.as_str());
    let pattern = build_match_pattern(pattern_pair);
    println!("Built pattern: {:?}", pattern);

    let mut guard = None;
    let mut body = Vec::new();
    
    let remaining_after_pattern = inner.clone().collect::<Vec<_>>();
    println!("Remaining pairs after pattern: {:?}", remaining_after_pattern.iter().map(|p| (p.as_rule(), p.as_str())).collect::<Vec<_>>());
    
    // Peek at the next significant token to check for "when"
    let mut peek_iter = inner.clone(); // Clone for peeking
    let next_token_peeked = next_significant(&mut peek_iter);

    if let Some(token_peeked) = next_token_peeked {
        println!("Next token after pattern (peeked): {:?} \'{}\'", token_peeked.as_rule(), token_peeked.as_str());
        if token_peeked.as_rule() == Rule::when_keyword {
            println!("Found \'when\' guard keyword");
            // Consume the 'when_keyword' from the original iterator 'inner'
            let _when_keyword_token = next_significant(&mut inner).expect("Consumed when keyword already identified by peeking"); 

            let guard_expr_pair = next_significant(&mut inner)
                .expect("\'when\' guard requires an expression");
            
            println!("Guard expression: {:?} \'{}\'", guard_expr_pair.as_rule(), guard_expr_pair.as_str());    
            guard = Some(Box::new(build_expression(guard_expr_pair.clone())));
            println!("Built guard expression: {:?}", guard);
            
            // All subsequent significant pairs are body expressions
            println!("Building body expressions after guard");
            while let Some(body_expr_pair) = next_significant(&mut inner) {
                println!("Body expression: {:?} \'{}\'", body_expr_pair.as_rule(), body_expr_pair.as_str());
                body.push(build_expression(body_expr_pair));
            }
        } else {
            // No "when" keyword, so the token_peeked is the first body expression.
            // We need to consume it from the original `inner` iterator.
            println!("No \'when\' found, processing as body expressions, starting with: {:?} \'{}\'", token_peeked.as_rule(), token_peeked.as_str());
            let first_body_expr_pair = next_significant(&mut inner).expect("Consumed first body expression already identified by peeking");
            body.push(build_expression(first_body_expr_pair)); 
            // Process remaining pairs from the original iterator 'inner'
            while let Some(body_expr_pair) = next_significant(&mut inner) {
                println!("Body expression: {:?} \'{}\'", body_expr_pair.as_rule(), body_expr_pair.as_str());
                body.push(build_expression(body_expr_pair));
            }
        }
    } else {
        println!("No tokens found after pattern - empty body and no guard.");
    }

    MatchClause {
        pattern,
        guard,
        body,
    }
}

// match_pattern = _{ literal | symbol | keyword | "_" | type_expr | vector_match_pattern | map_match_pattern | ("(" ~ ":as" ~ symbol ~ match_pattern ~ ")") }
fn build_match_pattern(pair: Pair<Rule>) -> MatchPattern {
    // Drill down if needed
    let actual_pair = match pair.as_rule() {
        Rule::match_pattern => pair.into_inner().next().unwrap(),
        _ => pair,
    };

    // Check for the literal "_" string first
    if actual_pair.as_str() == "_" {
        return MatchPattern::Wildcard;
    }

    match actual_pair.as_rule() {
        Rule::literal => MatchPattern::Literal(build_literal(actual_pair)),
        Rule::symbol => MatchPattern::Symbol(build_symbol(actual_pair)),
        Rule::keyword => MatchPattern::Keyword(build_keyword(actual_pair)),
        Rule::wildcard => MatchPattern::Wildcard,
        Rule::type_expr => MatchPattern::Type(build_type_expr(actual_pair)),
        Rule::vector_match_pattern => MatchPattern::Vector(build_vector_match_pattern(actual_pair)),
        Rule::map_match_pattern => MatchPattern::Map(build_map_match_pattern(actual_pair)),
        // Check for :as pattern structure: "(" ~ ":as" ~ symbol ~ match_pattern ~ ")"
        Rule::list if actual_pair.as_str().starts_with("(:as") => {
            let mut inner = actual_pair.into_inner();
            // Skip (:as keyword
            let _ = next_significant(&mut inner);
            let symbol = build_symbol(next_significant(&mut inner).expect(":as needs symbol"));
            let pattern =
                build_match_pattern(next_significant(&mut inner).expect(":as needs pattern"));
            MatchPattern::As(symbol, Box::new(pattern))
        }
        rule => unimplemented!(
            "build_match_pattern not implemented for rule: {:?} - {}",
            rule,
            actual_pair.as_str()
        ),
    }
}

// vector_match_pattern = { "[" ~ match_pattern* ~ ("&" ~ symbol)? ~ "]" }
fn build_vector_match_pattern(pair: Pair<Rule>) -> VectorMatchPattern {
    assert_eq!(pair.as_rule(), Rule::vector_match_pattern);
    let mut inner = pair.into_inner();
    let mut elements = Vec::new();
    let mut rest = None;

    while let Some(p) = next_significant(&mut inner) {
        if p.as_str() == "&" {
            rest = Some(build_symbol(
                next_significant(&mut inner).expect("Vector match pattern & needs symbol"),
            ));
            break; // Rest must be last
        } else if p.as_rule() == Rule::match_pattern {
            elements.push(build_match_pattern(p));
        } else {
            panic!("Unexpected rule in vector_match_pattern: {:?}", p.as_rule());
        }
    }
    VectorMatchPattern { elements, rest }
}

// map_match_pattern = { "{" ~ map_match_pattern_entry* ~ ("&" ~ symbol)? ~ "}" }
fn build_map_match_pattern(pair: Pair<Rule>) -> MapMatchPattern {
    assert_eq!(pair.as_rule(), Rule::map_match_pattern);
    let mut inner = pair.into_inner();
    let mut entries = Vec::new();
    let mut rest = None;

    while let Some(p) = next_significant(&mut inner) {
        if p.as_str() == "&" {
            rest = Some(build_symbol(
                next_significant(&mut inner).expect("Map match pattern & needs symbol"),
            ));
            break; // Rest must be last
        } else if p.as_rule() == Rule::map_match_pattern_entry {
            // map_match_pattern_entry = { map_key ~ match_pattern }
            let mut entry_inner = p.into_inner();
            let key =
                build_map_key(next_significant(&mut entry_inner).expect("Map entry needs key"));
            let pattern = build_match_pattern(
                next_significant(&mut entry_inner).expect("Map entry needs pattern"),
            );
            entries.push((key, pattern));
        } else {
            panic!("Unexpected rule in map_match_pattern: {:?}", p.as_rule());
        }
    }
    MapMatchPattern { entries, rest }
}

// log_step_expr = { "(" ~ log_step_keyword ~ ":id" ~ string ~ expression ~ ")" }
pub(super) fn build_log_step_expr(mut pairs: Pairs<Rule>) -> LogStepExpr {
    // Advance past any initial whitespace or comments
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // Consume
        } else {
            break;
        }
    }

    // If the first significant token is `log_step_keyword`, consume it.
    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::log_step_keyword {
            pairs.next(); // Consume the log_step_keyword
        }
    }
    
    // Get the string value for the id (":id" literal is consumed by grammar parser)
    let id_value_pair = pairs.next().expect("log-step expects string value for id");
    assert_eq!(id_value_pair.as_rule(), Rule::string, "Expected string for log-step id value");
    
    let id_str_raw = id_value_pair.as_str();

    // Remove quotes and unescape string literal
    let id_content = &id_str_raw[1..id_str_raw.len() - 1];
    let id = unescape(id_content).expect("Invalid escape sequence in log-step id string");
    
    // Get the expression
    let expression_pair = pairs.next().expect("log-step requires an expression");
    let expression = Box::new(build_expression(expression_pair));
    
    LogStepExpr { id, expression }
}

