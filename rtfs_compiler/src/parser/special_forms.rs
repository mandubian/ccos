use super::{PestParseError, Rule};
use pest::iterators::{Pair, Pairs};

// AST Node Imports - Ensure all used AST nodes are listed here
use crate::ast::{
    CatchClause,
    CatchPattern,
    DefExpr,
    DefnExpr,
    DoExpr,
    Expression, // Ensure this is correctly in scope
    FnExpr,
    IfExpr,
    LetBinding,
    LetExpr,
    LogStepExpr,
    MapMatchEntry,
    MatchClause,
    MatchExpr,
    MatchPattern,
    ParallelBinding,
    ParallelExpr,
    ParamDef,
    Pattern,
    Symbol,
    TryCatchExpr,
    TypeExpr,
    WithResourceExpr,
};

// Builder function imports from sibling modules
// CORRECTED IMPORT: build_keyword_from_pair -> build_keyword
use super::common::{build_keyword, build_literal, build_map_key, build_pattern, build_symbol};
use super::expressions::build_expression;
use super::types::build_type_expr; // For type annotations

// Utility imports (if any) - e.g., for skipping whitespace/comments if not handled by Pest rules
use super::utils::unescape; // For log_step_expr

pub(super) fn build_let_expr(pairs: Pairs<Rule>) -> Result<LetExpr, PestParseError> {
    let mut significant_pairs = pairs.peekable();

    // 1. Consume let_keyword if present
    while let Some(p) = significant_pairs.peek() {
        match p.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => {
                significant_pairs.next();
            }
            Rule::let_keyword => {
                significant_pairs.next();
                while let Some(sp) = significant_pairs.peek() {
                    if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                        significant_pairs.next();
                    } else {
                        break;
                    }
                }
                break;
            }
            _ => break,
        }
    }

    // 2. Get the bindings pair.
    let bindings_pair_outer = significant_pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("Let expression requires a binding vector".to_string())
    })?;

    let mut bindings = Vec::new();
    match bindings_pair_outer.as_rule() {
        Rule::vector => {
            let mut binding_content_pairs = bindings_pair_outer
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT);

            while let Some(pattern_candidate_pair) = binding_content_pairs.next() {
                let pattern = build_pattern(pattern_candidate_pair)?;

                let type_annotation: Option<TypeExpr> = None; // Placeholder

                let value_pair = binding_content_pairs.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Let binding vector expects pairs of pattern and value expressions (missing value)"
                            .to_string(),
                    )
                })?;
                let value = build_expression(value_pair)?;
                bindings.push(LetBinding {
                    pattern,
                    type_annotation,
                    value: Box::new(value),
                });
            }
        }
        actual_rule => {
            return Err(PestParseError::InvalidInput(format!(
                "Expected Rule::vector for let bindings, found {:?} (\'{}\')",
                actual_rule,
                bindings_pair_outer.as_str()
            )));
        }
    }

    // 3. The rest of significant_pairs are body expressions
    let body = significant_pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LetExpr { bindings, body })
}

pub(super) fn build_if_expr(mut pairs: Pairs<Rule>) -> Result<IfExpr, PestParseError> {
    let condition_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken("if condition".to_string()))?;
    let then_branch_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken("if then_branch".to_string()))?;

    let condition = Box::new(build_expression(condition_pair)?);
    let then_branch = Box::new(build_expression(then_branch_pair)?);
    let else_branch = pairs
        .next()
        .map(|p| build_expression(p).map(Box::new))
        .transpose()?;

    Ok(IfExpr {
        condition,
        then_branch,
        else_branch,
    })
}

pub(super) fn build_do_expr(pairs: Pairs<Rule>) -> Result<DoExpr, PestParseError> {
    let mut significant_pairs = pairs.peekable();

    while let Some(p) = significant_pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            significant_pairs.next();
        } else {
            break;
        }
    }

    if let Some(first_token) = significant_pairs.peek() {
        if first_token.as_rule() == Rule::do_keyword {
            significant_pairs.next();
        }
    }

    let expressions = significant_pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DoExpr { expressions })
}

pub(super) fn build_fn_expr(mut pairs: Pairs<Rule>) -> Result<FnExpr, PestParseError> {
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }

    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::fn_keyword {
            pairs.next();
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let params_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput("fn requires parameters list".to_string()))?;
    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput(format!(
            "Expected fn_param_list, found {:?}",
            params_pair.as_rule()
        )));
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE
            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }

        if param_item_peek.as_str() == "&" {
            params_inner.next();
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                    params_inner.next();
                } else {
                    break;
                }
            }
            let rest_symbol_pair = params_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput("& requires a symbol".to_string()))?;
            if rest_symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected symbol after &, found {:?}",
                    rest_symbol_pair.as_rule()
                )));
            }
            let rest_symbol = build_symbol(rest_symbol_pair)?;

            let mut rest_type_annotation = None;
            if let Some(peeked_type) = params_inner.peek() {
                if peeked_type.as_rule() == Rule::type_expr {
                    rest_type_annotation = Some(build_type_expr(params_inner.next().unwrap())?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_symbol),
                type_annotation: rest_type_annotation,
            });
            break;
        }

        // Regular parameter (Pattern + optional TypeExpr)
        let pattern_pair = params_inner.next().unwrap(); // Should be safe due to peek
        let pattern = build_pattern(pattern_pair)?;

        let mut type_annotation = None;
        if let Some(peeked_colon_or_type) = params_inner.peek() {
            if peeked_colon_or_type.as_rule() == Rule::COLON {
                params_inner.next(); // Consume \':\'
                                     // Consume potential whitespace after \':\'
                while let Some(p_ws) = params_inner.peek() {
                    if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                        params_inner.next();
                    } else {
                        break;
                    }
                }
                let type_pair = params_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Expected type_expr after \':\' in parameter".to_string(),
                    )
                })?;
                if type_pair.as_rule() != Rule::type_expr {
                    return Err(PestParseError::InvalidInput(format!(
                        "Expected type_expr after \':\', found {:?}",
                        type_pair.as_rule()
                    )));
                }
                type_annotation = Some(build_type_expr(type_pair)?);
            } else if peeked_colon_or_type.as_rule() == Rule::type_expr {
                // Direct type_expr without preceding COLON (if grammar allows)
                // This case might need grammar adjustment if COLON is mandatory
                type_annotation = Some(build_type_expr(params_inner.next().unwrap())?);
            }
        }
        params.push(ParamDef {
            pattern,
            type_annotation,
        });
    }

    // Optional return type
    let mut return_type: Option<TypeExpr> = None;
    if let Some(peeked_ret_colon) = pairs.peek() {
        if peeked_ret_colon.as_rule() == Rule::COLON {
            pairs.next(); // Consume \':\'
                          // Consume potential whitespace after \':\'
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
            let return_type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput(
                    "Expected type_expr after \':\' for return type".to_string(),
                )
            })?;
            if return_type_pair.as_rule() != Rule::type_expr {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected type_expr for return type, found {:?}",
                    return_type_pair.as_rule()
                )));
            }
            return_type = Some(build_type_expr(return_type_pair)?);
        }
    }

    // Body expressions
    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "fn requires at least one body expression".to_string(),
        ));
    }

    Ok(FnExpr {
        params,
        variadic_param,
        body,
        return_type,
        // name: None, // REMOVED: FnExpr AST does not have a name field
    })
}

pub(super) fn build_def_expr(mut pairs: Pairs<Rule>) -> Result<DefExpr, PestParseError> {
    // Consume def_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::def_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let symbol_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput("def requires a symbol".to_string()))?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for def, found {:?}",
            symbol_pair.as_rule()
        )));
    }
    let symbol = build_symbol(symbol_pair)?;

    // Optional type annotation
    let mut type_annotation: Option<TypeExpr> = None;
    if let Some(peeked_colon) = pairs.peek() {
        if peeked_colon.as_rule() == Rule::COLON {
            pairs.next(); // Consume \':\'
                          // Consume potential whitespace after \':\'
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
            let type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("Expected type_expr after \':\' in def".to_string())
            })?;
            if type_pair.as_rule() != Rule::type_expr {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected type_expr for def annotation, found {:?}",
                    type_pair.as_rule()
                )));
            }
            type_annotation = Some(build_type_expr(type_pair)?);
        }
    }

    let value_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("def requires a value expression".to_string())
    })?;
    let value = build_expression(value_pair)?;

    Ok(DefExpr {
        symbol,
        type_annotation,
        value: Box::new(value),
    })
}

pub(super) fn build_defn_expr(mut pairs: Pairs<Rule>) -> Result<DefnExpr, PestParseError> {
    // Consume defn_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::defn_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let symbol_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("defn requires a symbol (function name)".to_string())
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for defn name, found {:?}",
            symbol_pair.as_rule()
        )));
    }
    let name = build_symbol(symbol_pair)?;

    // The rest of the pairs form the fn_expr (params, body, etc.)
    // We need to reconstruct a Pairs<Rule> for build_fn_expr.
    // This is a bit tricky as `pairs` is an iterator.
    // A simpler approach might be to expect build_fn_expr to handle pairs that *start* with params_list.
    // Or, we parse params, return type, and body here directly.
    // For now, let\'s assume build_fn_expr can take the remaining pairs.
    // However, build_fn_expr expects to be able to consume an optional fn_keyword.
    // To make this work, we pass the *original* `pair.into_inner()` to `build_fn_expr`
    // and it skips the `defn_keyword` and `symbol`.
    // This is not ideal. Let\'s parse fn components here.

    let params_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput("defn requires parameters list".to_string()))?;
    // `build_fn_expr` expects pairs starting with fn_keyword or params_list.
    // We need to create a new `Pairs` that `build_fn_expr` can consume.
    // This is complex. Let\'s simplify: defn will directly parse fn components.

    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput(format!(
            "Expected fn_param_list for defn, found {:?}",
            params_pair.as_rule()
        )));
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE
            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }
        if param_item_peek.as_str() == "&" {
            params_inner.next(); // &
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    params_inner.next();
                } else {
                    break;
                }
            }
            let rest_sym_pair = params_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("defn: & requires symbol".to_string())
            })?;
            let rest_sym = build_symbol(rest_sym_pair)?;
            let mut rest_type: Option<TypeExpr> = None;
            if let Some(peek_type) = params_inner.peek() {
                if peek_type.as_rule() == Rule::type_expr {
                    rest_type = Some(build_type_expr(params_inner.next().unwrap())?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_sym),
                type_annotation: rest_type,
            });
            break;
        }
        let pat_pair = params_inner.next().unwrap();
        let pattern = build_pattern(pat_pair)?;
        let mut type_ann: Option<TypeExpr> = None;
        if let Some(peek_colon) = params_inner.peek() {
            if peek_colon.as_rule() == Rule::COLON {
                params_inner.next(); // :
                while let Some(p) = params_inner.peek() {
                    if p.as_rule() == Rule::WHITESPACE {
                        params_inner.next();
                    } else {
                        break;
                    }
                }
                let type_pair = params_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput("defn: expected type after :".to_string())
                })?;
                type_ann = Some(build_type_expr(type_pair)?);
            } else if peek_colon.as_rule() == Rule::type_expr {
                // type without colon
                type_ann = Some(build_type_expr(params_inner.next().unwrap())?);
            }
        }
        params.push(ParamDef {
            pattern,
            type_annotation: type_ann,
        });
    }

    let mut return_type: Option<TypeExpr> = None;
    if let Some(peek_ret_colon) = pairs.peek() {
        if peek_ret_colon.as_rule() == Rule::COLON {
            pairs.next(); // :
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    pairs.next();
                } else {
                    break;
                }
            }
            let ret_type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("defn: expected return type after :".to_string())
            })?;
            return_type = Some(build_type_expr(ret_type_pair)?);
        }
    }

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "defn requires at least one body expression".to_string(),
        ));
    }

    Ok(DefnExpr {
        name,
        params,
        variadic_param,
        body,
        return_type,
    })
}

pub(super) fn build_parallel_expr(mut pairs: Pairs<Rule>) -> Result<ParallelExpr, PestParseError> {
    // Consume parallel_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::parallel_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let bindings_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("parallel expression requires a binding vector".to_string())
    })?;
    if bindings_pair.as_rule() != Rule::vector {
        return Err(PestParseError::InvalidInput(format!(
            "Expected Rule::vector for parallel bindings, found {:?}",
            bindings_pair.as_rule()
        )));
    }

    let mut bindings = Vec::new();
    // CORRECTED: make the filtered iterator peekable
    let mut binding_content_pairs = bindings_pair
        .into_inner()
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .peekable();

    while let Some(symbol_pair) = binding_content_pairs.next() {
        if symbol_pair.as_rule() != Rule::symbol {
            return Err(PestParseError::InvalidInput(format!(
                "Expected symbol for parallel binding, found {:?}",
                symbol_pair.as_rule()
            )));
        }
        let symbol = build_symbol(symbol_pair)?;

        // Optional type annotation for parallel binding
        let mut type_annotation: Option<TypeExpr> = None;
        if let Some(peeked_type_ann) = binding_content_pairs.peek() {
            if peeked_type_ann.as_rule() == Rule::type_annotation {
                let type_ann_pair = binding_content_pairs.next().unwrap(); // Consume type_annotation
                let type_expr_pair = type_ann_pair
                    .into_inner()
                    .next() // Skip COLON
                    .ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "Parallel binding type_annotation missing type_expr".to_string(),
                        )
                    })?;
                type_annotation = Some(build_type_expr(type_expr_pair)?);
            }
        }

        let expr_pair = binding_content_pairs.next().ok_or_else(|| {
            // Changed from value_pair
            PestParseError::InvalidInput(
                "Parallel binding vector expects pairs of symbol and value".to_string(),
            )
        })?;
        let expression = build_expression(expr_pair)?; // Changed from value
        bindings.push(ParallelBinding {
            symbol,
            type_annotation,
            expression: Box::new(expression), // CHANGED: from value to expression
        });
    }
    Ok(ParallelExpr { bindings })
}

pub(super) fn build_with_resource_expr(
    mut pairs: Pairs<Rule>,
) -> Result<WithResourceExpr, PestParseError> {
    // Consume with_resource_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::with_resource_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let resource_binding_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource requires a resource binding".to_string())
    })?;
    if resource_binding_pair.as_rule() != Rule::vector
        || resource_binding_pair
            .clone()
            .into_inner()
            .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .count()
            != 2
    {
        return Err(PestParseError::InvalidInput(
            "with-resource binding must be a vector of [symbol expression]".to_string(),
        ));
    }
    // The grammar is: \"[\" ~ symbol ~ type_expr ~ expression ~ \"]\"
    // The AST is: resource_symbol, resource_type, resource_init
    let mut binding_inner = resource_binding_pair
        .into_inner()
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT);
    let symbol_pair = binding_inner.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource binding missing symbol".to_string())
    })?;
    let type_expr_pair = binding_inner.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource binding missing type_expr".to_string())
    })?;
    let resource_init_pair = binding_inner.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource binding missing expression".to_string())
    })?;

    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for with-resource binding, found {:?}",
            symbol_pair.as_rule()
        )));
    }
    let symbol = build_symbol(symbol_pair)?;
    let resource_type = build_type_expr(type_expr_pair)?;
    let resource_init_expr = build_expression(resource_init_pair)?; // CHANGED: from resource_expr

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "with-resource requires at least one body expression".to_string(),
        ));
    }

    Ok(WithResourceExpr {
        resource_symbol: symbol,
        resource_type,
        resource_init: Box::new(resource_init_expr), // CHANGED: from resource_expression to resource_init
        body,
    })
}

pub(super) fn build_try_catch_expr(mut pairs: Pairs<Rule>) -> Result<TryCatchExpr, PestParseError> {
    // Consume try_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::try_keyword {
            // Assuming try_keyword exists
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    // The first expression is the try_body (grammar uses try_body_expression+)
    // The AST uses try_body: Vec<Expression>
    // We need to collect all try_body_expression before catch_clauses or finally_clause
    let mut try_body_expressions = Vec::new();
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::try_body_expression {
            try_body_expressions.push(build_expression(pairs.next().unwrap())?);
        } else if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next(); // consume whitespace/comment
        } else {
            break; // End of try_body_expressions
        }
    }

    if try_body_expressions.is_empty() {
        return Err(PestParseError::InvalidInput(
            "try-catch requires a try block expression".to_string(),
        ));
    }

    // The rest are catch_clauses and an optional finally_clause
    let mut catch_clauses = Vec::new();
    let mut finally_body: Option<Vec<Expression>> = None; // Added

    while let Some(clause_candidate_pair) = pairs.peek() {
        // Skip whitespace/comments
        if clause_candidate_pair.as_rule() == Rule::WHITESPACE
            || clause_candidate_pair.as_rule() == Rule::COMMENT
        {
            pairs.next(); // consume
            continue;
        }

        if clause_candidate_pair.as_rule() == Rule::catch_clause {
            let catch_clause_pair = pairs.next().unwrap(); // Safe due to peek and check
            let mut clause_inner = catch_clause_pair.into_inner();

            // Consume catch_keyword
            let _catch_keyword_pair = clause_inner
                .next()
                .filter(|p| p.as_rule() == Rule::catch_keyword)
                .ok_or_else(|| {
                    PestParseError::InvalidInput("Catch clause missing 'catch' keyword".to_string())
                })?;

            let pattern_pair = clause_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("Catch clause requires a pattern".to_string())
            })?;
            // catch_pattern = _{ type_expr | keyword | symbol }
            // The AST CatchPattern has variants for these. build_catch_pattern handles this.
            let pattern = build_catch_pattern(pattern_pair)?;

            let binding_symbol_pair = clause_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("Catch clause requires a binding symbol".to_string())
            })?;
            if binding_symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected symbol for catch binding, found {:?}",
                    binding_symbol_pair.as_rule()
                )));
            }
            let binding = build_symbol(binding_symbol_pair)?;

            // Collect all body expressions for the catch clause
            let mut catch_body_expressions = Vec::new();
            while let Some(body_expr_pair) = clause_inner.next() {
                if body_expr_pair.as_rule() == Rule::WHITESPACE
                    || body_expr_pair.as_rule() == Rule::COMMENT
                {
                    continue;
                }
                catch_body_expressions.push(build_expression(body_expr_pair)?);
            }
            if catch_body_expressions.is_empty() {
                return Err(PestParseError::InvalidInput(
                    "Catch clause requires at least one body expression".to_string(),
                ));
            }

            catch_clauses.push(CatchClause {
                pattern,
                binding,
                body: catch_body_expressions, // CHANGED: from handler_expression (Box<Expression>) to Vec<Expression>
            });
        } else if clause_candidate_pair.as_rule() == Rule::finally_clause {
            if finally_body.is_some() {
                return Err(PestParseError::InvalidInput(
                    "Multiple finally clauses found".to_string(),
                ));
            }
            let finally_clause_pair = pairs.next().unwrap();
            let mut finally_inner = finally_clause_pair.into_inner();
            // Consume finally_keyword
            let _finally_keyword_pair = finally_inner
                .next()
                .filter(|p| p.as_rule() == Rule::finally_keyword)
                .ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Finally clause missing 'finally' keyword".to_string(),
                    )
                })?;

            let mut finally_expressions = Vec::new();
            while let Some(body_expr_pair) = finally_inner.next() {
                // consume remaining pairs as expressions
                if body_expr_pair.as_rule() == Rule::WHITESPACE
                    || body_expr_pair.as_rule() == Rule::COMMENT
                {
                    continue;
                }
                finally_expressions.push(build_expression(body_expr_pair)?);
            }
            if finally_expressions.is_empty() {
                return Err(PestParseError::InvalidInput(
                    "Finally clause requires at least one body expression".to_string(),
                ));
            }
            finally_body = Some(finally_expressions);
        } else {
            // Should not happen if grammar is correct and all try_body_expressions consumed
            return Err(PestParseError::InvalidInput(format!(
                "Expected catch_clause or finally_clause, found {:?} in try-catch",
                clause_candidate_pair.as_rule()
            )));
        }
    }

    // As per grammar, catch_clause* means zero or more.
    // But AST TryCatchExpr implies try_body and catch_clauses are somewhat mandatory.
    // The prompt error "missing structure fields: try_body, finally_body" for TryCatchExpr
    // and "missing structure fields: binding, body" for CatchClause.
    // Let's ensure try_body is passed. finally_body is Option.

    Ok(TryCatchExpr {
        try_body: try_body_expressions, // CHANGED: from try_block
        catch_clauses,
        finally_body,
    })
}

// build_catch_pattern needs to align with AST CatchPattern and Pest catch_pattern rule
// catch_pattern  = _{ type_expr | keyword | symbol }
// AST: enum CatchPattern { Keyword(Keyword), Type(TypeExpr), Symbol(Symbol) }
fn build_catch_pattern(pair: Pair<Rule>) -> Result<CatchPattern, PestParseError> {
    match pair.as_rule() {
        Rule::type_expr => Ok(CatchPattern::Type(build_type_expr(pair)?)),
        Rule::keyword => Ok(CatchPattern::Keyword(build_keyword(pair)?)), // CORRECTED: ? after function call
        Rule::symbol => Ok(CatchPattern::Symbol(build_symbol(pair)?)), // CORRECTED: ? after function call
        unknown_rule => Err(PestParseError::InvalidInput(format!(
            "Invalid rule for catch_pattern: {:?}, content: '{}'",
            unknown_rule,
            pair.as_str()
        ))),
    }
}

pub(super) fn build_match_expr(mut pairs: Pairs<Rule>) -> Result<MatchExpr, PestParseError> {
    // Consume match_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::match_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let expression_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput(
            "match expression requires an expression to match against".to_string(),
        )
    })?;
    let expression = Box::new(build_expression(expression_pair)?);

    let mut clauses = Vec::new();
    while let Some(clause_pair_candidate) = pairs.peek() {
        if clause_pair_candidate.as_rule() == Rule::WHITESPACE
            || clause_pair_candidate.as_rule() == Rule::COMMENT
        {
            pairs.next();
            continue;
        }
        if clause_pair_candidate.as_rule() != Rule::match_clause {
            return Err(PestParseError::InvalidInput(format!(
                "Expected match_clause, found {:?} in match expression",
                clause_pair_candidate.as_rule()
            )));
        }
        let clause_pair = pairs.next().unwrap(); // Safe due to peek and check

        let mut clause_inner = clause_pair.into_inner();
        let pattern_pair = clause_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput("Match clause requires a pattern".to_string())
        })?;
        let pattern = build_match_pattern(pattern_pair)?;

        // Optional guard: (when_keyword ~ expression)?
        let mut guard: Option<Box<Expression>> = None;
        if let Some(peeked) = clause_inner.peek() {
            if peeked.as_rule() == Rule::when_keyword {
                clause_inner.next(); // Consume when_keyword
                                     // Consume potential whitespace
                while let Some(ws) = clause_inner.peek() {
                    if ws.as_rule() == Rule::WHITESPACE || ws.as_rule() == Rule::COMMENT {
                        clause_inner.next();
                    } else {
                        break;
                    }
                }
                let guard_expr_pair = clause_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Match clause guard missing expression after 'when'".to_string(),
                    )
                })?;
                guard = Some(Box::new(build_expression(guard_expr_pair)?));
            }
        }

        // Body expressions (expression+)
        let mut body_expressions = Vec::new();
        while let Some(body_expr_pair) = clause_inner.next() {
            if body_expr_pair.as_rule() == Rule::WHITESPACE
                || body_expr_pair.as_rule() == Rule::COMMENT
            {
                continue;
            }
            body_expressions.push(build_expression(body_expr_pair)?);
        }
        if body_expressions.is_empty() {
            return Err(PestParseError::InvalidInput(
                "Match clause requires at least one body expression".to_string(),
            ));
        }

        clauses.push(MatchClause {
            pattern,
            guard,
            body: body_expressions, // CHANGED: from result_expression (Box<Expression>) to Vec<Expression>
        });
    }

    if clauses.is_empty() {
        return Err(PestParseError::InvalidInput(
            "match expression requires at least one clause".to_string(),
        ));
    }

    Ok(MatchExpr {
        expression,
        clauses,
    })
}

// Refined build_match_pattern based on rtfs.pest:
// match_pattern = _{ literal | symbol | keyword | "_" | type_expr | vector_match_pattern | map_match_pattern | ("(" ~ ":as" ~ symbol ~ match_pattern ~ ")") }
fn build_match_pattern(pair: Pair<Rule>) -> Result<MatchPattern, PestParseError> {
    match pair.as_rule() {
        Rule::literal => Ok(MatchPattern::Literal(build_literal(pair)?)), // CORRECTED: ? after function call
        Rule::symbol => Ok(MatchPattern::Symbol(build_symbol(pair)?)), // CORRECTED: ? after function call
        Rule::keyword => Ok(MatchPattern::Keyword(build_keyword(pair)?)), // CORRECTED: ? after function call
        Rule::wildcard => Ok(MatchPattern::Wildcard),
        Rule::type_expr => Ok(MatchPattern::Type(build_type_expr(pair)?, None)),
        Rule::vector_match_pattern => {
            let mut elements = Vec::new();
            let mut rest: Option<Symbol> = None;
            let mut inner_pairs = pair.into_inner().peekable(); // Use peekable for peeking
            while let Some(p_peek) = inner_pairs.peek() {
                // Peek before consuming
                if p_peek.as_rule() == Rule::WHITESPACE || p_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next(); // Consume whitespace/comment
                    continue;
                }
                // Check for rest pattern string "&"
                if p_peek.as_str() == "&" {
                    inner_pairs.next(); // Consume "&"
                                        // Consume optional whitespace after &
                    while let Some(ws_peek) = inner_pairs.peek() {
                        if ws_peek.as_rule() == Rule::WHITESPACE {
                            inner_pairs.next();
                        } else {
                            break;
                        }
                    }
                    let sym_pair = inner_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "Expected symbol after & in vector_match_pattern".to_string(),
                        )
                    })?;
                    if sym_pair.as_rule() != Rule::symbol {
                        return Err(PestParseError::InvalidInput(format!(
                            "Expected symbol for vector rest, found {:?}, content: '{}'",
                            sym_pair.as_rule(),
                            sym_pair.as_str()
                        )));
                    }
                    rest = Some(build_symbol(sym_pair)?);
                    break;
                }
                // If not whitespace, comment, or rest pattern, it must be an element pattern
                let p = inner_pairs.next().unwrap(); // Consume the pattern element
                elements.push(build_match_pattern(p)?);
            }
            Ok(MatchPattern::Vector { elements, rest })
        }
        Rule::map_match_pattern => {
            let mut entries = Vec::new();
            let mut rest: Option<Symbol> = None;
            let mut inner_pairs = pair.into_inner().peekable(); // Use peekable for peeking
            while let Some(p_peek) = inner_pairs.peek() {
                // Peek before consuming
                if p_peek.as_rule() == Rule::WHITESPACE || p_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next(); // Consume whitespace/comment
                    continue;
                }
                // Check for rest pattern string "&"
                if p_peek.as_str() == "&" {
                    inner_pairs.next(); // Consume "&"
                                        // Consume optional whitespace after &
                    while let Some(ws_peek) = inner_pairs.peek() {
                        if ws_peek.as_rule() == Rule::WHITESPACE {
                            inner_pairs.next();
                        } else {
                            break;
                        }
                    }
                    let sym_pair = inner_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "Expected symbol after & in map_match_pattern".to_string(),
                        )
                    })?;
                    if sym_pair.as_rule() != Rule::symbol {
                        return Err(PestParseError::InvalidInput(format!(
                            "Expected symbol for map rest, found {:?}, content: '{}'",
                            sym_pair.as_rule(),
                            sym_pair.as_str()
                        )));
                    }
                    rest = Some(build_symbol(sym_pair)?);
                    break;
                }
                // If not whitespace, comment, or rest pattern, it must be a map entry
                let entry_pair = inner_pairs.next().unwrap(); // Consume the map entry
                if entry_pair.as_rule() != Rule::map_match_pattern_entry {
                    return Err(PestParseError::InvalidInput(format!("Expected map_match_pattern_entry in map pattern, found {:?}, content: '{}'", entry_pair.as_rule(), entry_pair.as_str())));
                }
                let mut entry_inner = entry_pair.into_inner();
                let key_pair = entry_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput("Map match entry missing key".to_string())
                })?;
                let value_pattern_pair = entry_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Map match entry missing value pattern".to_string(),
                    )
                })?;

                entries.push(MapMatchEntry {
                    key: build_map_key(key_pair)?,
                    pattern: Box::new(build_match_pattern(value_pattern_pair)?),
                });
            }
            Ok(MatchPattern::Map { entries, rest })
        }
        Rule::list => {
            let mut inner_list_pairs = pair.clone().into_inner().peekable();
            if let Some(first_in_list) = inner_list_pairs.peek() {
                if first_in_list.as_str() == ":as" {
                    inner_list_pairs.next(); // Consume :as

                    while let Some(ws_peek) = inner_list_pairs.peek() {
                        // Consume whitespace after :as
                        if ws_peek.as_rule() == Rule::WHITESPACE
                            || ws_peek.as_rule() == Rule::COMMENT
                        {
                            inner_list_pairs.next();
                        } else {
                            break;
                        }
                    }

                    let symbol_pair = inner_list_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "AS pattern: missing symbol after :as".to_string(),
                        )
                    })?;
                    if symbol_pair.as_rule() != Rule::symbol {
                        return Err(PestParseError::InvalidInput(format!(
                            "Expected symbol for AS pattern, found {:?}, content: '{}'",
                            symbol_pair.as_rule(),
                            symbol_pair.as_str()
                        )));
                    }
                    let symbol = build_symbol(symbol_pair)?;

                    while let Some(ws_peek) = inner_list_pairs.peek() {
                        // Consume whitespace after symbol
                        if ws_peek.as_rule() == Rule::WHITESPACE
                            || ws_peek.as_rule() == Rule::COMMENT
                        {
                            inner_list_pairs.next();
                        } else {
                            break;
                        }
                    }

                    let pattern_to_bind_pair = inner_list_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "AS pattern: missing pattern to bind".to_string(),
                        )
                    })?;
                    let pattern_to_bind = build_match_pattern(pattern_to_bind_pair)?;

                    if let Some(extra_peek) = inner_list_pairs.peek() {
                        // Check for extra tokens
                        if extra_peek.as_rule() != Rule::WHITESPACE
                            && extra_peek.as_rule() != Rule::COMMENT
                        {
                            return Err(PestParseError::InvalidInput(format!(
                                "AS pattern: unexpected token after pattern: {:?}, content: '{}'",
                                extra_peek.as_rule(),
                                extra_peek.as_str()
                            )));
                        }
                    }
                    return Ok(MatchPattern::As(symbol, Box::new(pattern_to_bind)));
                }
            }
            Err(PestParseError::UnsupportedRule(format!(
                "build_match_pattern: Unexpected list content: '{}'. Expected '(:as symbol pattern)' or other known match pattern rule.",
                pair.as_str()
            )))
        }
        unknown_rule => {
            // Handle '_' for wildcard if it's not a specific Rule::wildcard
            if pair.as_str() == "_" {
                return Ok(MatchPattern::Wildcard);
            }
            Err(PestParseError::UnsupportedRule(format!(
                "build_match_pattern: Unhandled rule: {:?}, content: '{}'",
                unknown_rule,
                pair.as_str()
            )))
        }
    }
}

pub(super) fn build_log_step_expr(mut pairs: Pairs<Rule>) -> Result<LogStepExpr, PestParseError> {
    // Consume log_step_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::log_step_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let label_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("log-step requires a label (string)".to_string())
    })?;
    if label_pair.as_rule() != Rule::string {
        return Err(PestParseError::InvalidInput(format!(
            "Expected string for log-step label, found {:?}",
            label_pair.as_rule()
        )));
    }

    // Extract string content and unescape it properly
    let raw_str = label_pair.as_str();
    let content = &raw_str[1..raw_str.len() - 1]; // Remove quotes
    let label = unescape(content).map_err(|e| {
        PestParseError::InvalidEscapeSequence(format!(
            "Invalid string literal for log-step label: {:?}",
            e
        ))
    })?;

    let expression_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("log-step requires an expression".to_string())
    })?;
    let expression = build_expression(expression_pair)?;

    // Note: LogStepExpr structure doesn't match what's being built here
    // This is a structural issue beyond PestParseError fixes
    Ok(LogStepExpr {
        level: None,
        values: vec![expression],
        location: Some(label),
    })
}
