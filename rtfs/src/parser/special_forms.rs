use super::errors::{pair_to_source_span, PestParseError};
use super::expressions::build_expression;
use super::Rule;
use crate::ast::{Expression, MapKey};
use pest::iterators::{Pair, Pairs};
use std::collections::HashMap;

// AST Node Imports - Ensure all used AST nodes are listed here
use crate::ast::{
    CatchClause, CatchPattern, DefExpr, DefnExpr, DefstructExpr, DefstructField, DelegationHint,
    DoExpr, FnExpr, IfExpr, LetBinding, LetExpr, MatchClause, MatchExpr,
    ParamDef, Pattern, TryCatchExpr, TypeExpr,
};

// Builder function imports from sibling modules
// CORRECTED IMPORT: build_keyword_from_pair -> build_keyword
use super::common::{build_keyword, build_match_pattern, build_pattern, build_symbol};
use super::types::build_type_expr; // For type annotations

// Utility imports (if any) - e.g., for skipping whitespace/comments if not handled by Pest rules
// use super::utils::unescape; // For log_step_expr

pub(super) fn build_let_expr(pair: Pair<Rule>) -> Result<LetExpr, PestParseError> {
    let span = pair_to_source_span(&pair);
    let mut iter = pair.into_inner().peekable();
    let mut bindings = Vec::new();
    let mut body_expressions = Vec::new();

    // Skip the let_keyword if present
    if let Some(p) = iter.peek() {
        if p.as_rule() == Rule::let_keyword {
            iter.next();
        }
    }

    // Parse let_binding tokens
    for pair in iter {
        match pair.as_rule() {
            Rule::let_binding => {
                let pair_clone = pair.clone();
                let binding = build_let_binding(&pair, pair_clone.into_inner())?;
                bindings.push(binding);
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                // Skip whitespace and comments
            }
            _ => {
                // This should be a body expression
                let expr = build_expression(pair)?;
                body_expressions.push(expr);
            }
        }
    }
    if body_expressions.is_empty() {
        return Err(PestParseError::InvalidInput {
            message: "let expression requires at least one body expression".to_string(),
            span: Some(span),
        });
    }

    Ok(LetExpr {
        bindings,
        body: body_expressions,
    })
}

fn build_let_binding(
    parent_pair: &Pair<Rule>,
    mut pairs: Pairs<Rule>,
) -> Result<LetBinding, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let pattern_pair = pairs.next().ok_or_else(|| PestParseError::MissingToken {
        token: "let_binding pattern".to_string(),
        span: Some(parent_span.clone()),
    })?;

    let pattern = build_pattern(pattern_pair)?;

    // Check if there's a type annotation
    let mut type_annotation = None;
    let mut value_pair = None;

    if let Some(next_pair) = pairs.next() {
        if next_pair.as_rule() == Rule::type_annotation {
            // Parse type annotation
            let type_ann_inner = next_pair.into_inner();
            for token in type_ann_inner {
                match token.as_rule() {
                    Rule::COLON => continue, // Skip the colon
                    Rule::primitive_type
                    | Rule::vector_type
                    | Rule::tuple_type
                    | Rule::map_type
                    | Rule::function_type
                    | Rule::resource_type
                    | Rule::union_type
                    | Rule::intersection_type
                    | Rule::literal_type
                    | Rule::symbol => {
                        type_annotation = Some(build_type_expr(token)?);
                        break;
                    }
                    _ => continue,
                }
            }
            // The next token should be the expression
            value_pair = pairs.next();
        } else {
            // No type annotation, this is the expression
            value_pair = Some(next_pair);
        }
    }
    let value_pair = value_pair.ok_or_else(|| PestParseError::MissingToken {
        token: "let_binding value".to_string(),
        span: Some(parent_span),
    })?;

    let value = Box::new(build_expression(value_pair)?);

    Ok(LetBinding {
        pattern,
        type_annotation,
        value,
    })
}

pub(super) fn build_if_expr(pair: Pair<Rule>) -> Result<IfExpr, PestParseError> {
    let parent_span = pair_to_source_span(&pair);
    let mut pairs = pair.into_inner();
    let condition_pair = pairs.next().ok_or_else(|| PestParseError::MissingToken {
        token: "if condition".to_string(),
        span: Some(parent_span.clone()),
    })?;
    let then_branch_pair = pairs.next().ok_or_else(|| PestParseError::MissingToken {
        token: "if then_branch".to_string(),
        span: Some(parent_span.clone()),
    })?;

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

pub(super) fn build_fn_expr(pair: Pair<Rule>) -> Result<FnExpr, PestParseError> {
    let parent_span = pair_to_source_span(&pair);
    let mut pairs = pair.into_inner();
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

    // ---------------------------------------------------------
    // Parse optional metadata before parameter list
    // ---------------------------------------------------------
    let mut delegation_hint: Option<DelegationHint> = None;
    loop {
        // Skip whitespace/comments
        while let Some(p) = pairs.peek() {
            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                pairs.next();
            } else {
                break;
            }
        }

        let peek_pair = pairs.peek().ok_or_else(|| PestParseError::InvalidInput {
            message: "fn requires parameter list".to_string(),
            span: Some(parent_span.clone()),
        })?;

        match peek_pair.as_rule() {
            Rule::metadata => {
                let meta_pair = pairs.next().unwrap();
                let meta_span = pair_to_source_span(&meta_pair);
                // Find the delegation_meta within metadata
                let delegation_meta_pair = meta_pair
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::delegation_meta)
                    .ok_or_else(|| PestParseError::InvalidInput {
                        message: "metadata must contain delegation_meta".to_string(),
                        span: Some(meta_span),
                    })?;
                delegation_hint = Some(parse_delegation_meta(delegation_meta_pair)?);
                continue;
            }
            Rule::fn_param_list => {
                break;
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
                continue;
            }
            other => {
                return Err(PestParseError::InvalidInput {
                    message: format!("Unexpected token {:?} before fn param list", other),
                    span: Some(pair_to_source_span(&peek_pair.clone())),
                });
            }
        }
    }

    let params_pair = pairs.next().unwrap(); // Safe: we peeked it above
    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput {
            message: format!("Expected fn_param_list, found {:?}", params_pair.as_rule()),
            span: Some(pair_to_source_span(&params_pair)),
        });
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
        if param_item_peek.as_rule() == Rule::AMPERSAND {
            params_inner.next();
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                    params_inner.next();
                } else {
                    break;
                }
            }
            let rest_symbol_pair =
                params_inner
                    .next()
                    .ok_or_else(|| PestParseError::InvalidInput {
                        message: "& requires a symbol".to_string(),
                        span: Some(parent_span.clone()),
                    })?;
            if rest_symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected symbol after &, found {:?}",
                        rest_symbol_pair.as_rule()
                    ),
                    span: Some(pair_to_source_span(&rest_symbol_pair)),
                });
            }
            let rest_symbol = build_symbol(rest_symbol_pair)?;

            let mut rest_type_annotation = None;
            if let Some(peeked_colon) = params_inner.peek() {
                if peeked_colon.as_rule() == Rule::COLON {
                    params_inner.next(); // consume COLON
                                         // Consume potential whitespace after ':'
                    while let Some(p_ws) = params_inner.peek() {
                        if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                            params_inner.next();
                        } else {
                            break;
                        }
                    }
                    let type_pair =
                        params_inner
                            .next()
                            .ok_or_else(|| PestParseError::InvalidInput {
                                message: "Expected type_expr after ':' for variadic parameter"
                                    .to_string(),
                                span: Some(parent_span.clone()),
                            })?;
                    rest_type_annotation = Some(build_type_expr(type_pair)?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_symbol),
                type_annotation: rest_type_annotation,
            });
            break;
        } // Regular parameter (param_def contains binding_pattern and optional type)
        let param_def_pair = params_inner.next().unwrap();
        let param_def_span = pair_to_source_span(&param_def_pair);
        if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput {
                message: format!("Expected param_def, found {:?}", param_def_pair.as_rule()),
                span: Some(param_def_span.clone()),
            });
        }

        // Extract binding_pattern and optional type from param_def
        let mut param_def_inner = param_def_pair.into_inner();

        let binding_pattern_pair =
            param_def_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "param_def missing binding_pattern".to_string(),
                    span: Some(param_def_span.clone()),
                })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        // Check for optional type annotation (COLON ~ type_expr)
        let mut type_annotation = None;
        if let Some(colon_pair) = param_def_inner.next() {
            if colon_pair.as_rule() == Rule::COLON {
                // Get the type_expr after the colon
                let type_pair =
                    param_def_inner
                        .next()
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "Expected type_expr after ':' in param_def".to_string(),
                            span: Some(param_def_span.clone()),
                        })?;
                type_annotation = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected COLON in param_def, found {:?}",
                        colon_pair.as_rule()
                    ),
                    span: Some(pair_to_source_span(&colon_pair)),
                });
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
            let return_type_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
                message: "Expected type_expr after ':' for return type".to_string(),
                span: Some(parent_span.clone()),
            })?;
            return_type = Some(build_type_expr(return_type_pair)?);
        }
    }

    // Body expressions
    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if body.is_empty() {
        return Err(PestParseError::InvalidInput {
            message: "fn requires at least one body expression".to_string(),
            span: Some(parent_span),
        });
    }

    Ok(FnExpr {
        params,
        variadic_param,
        body,
        return_type,
        delegation_hint,
    })
}

pub(super) fn build_def_expr(def_expr_pair: Pair<Rule>) -> Result<DefExpr, PestParseError> {
    let def_span = pair_to_source_span(&def_expr_pair);
    let mut pairs = def_expr_pair.clone().into_inner();

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

    let symbol_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "def requires a symbol".to_string(),
        span: Some(def_span.clone()),
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput {
            message: format!("Expected symbol for def, found {:?}", symbol_pair.as_rule()),
            span: Some(pair_to_source_span(&symbol_pair)),
        });
    }
    let symbol = build_symbol(symbol_pair.clone())?;

    // Optional type annotation
    let mut type_annotation: Option<TypeExpr> = None;
    if let Some(peeked_colon) = pairs.peek() {
        if peeked_colon.as_rule() == Rule::COLON {
            let colon_pair = pairs.next().unwrap();
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
            let type_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
                message: "Expected type_expr after ':' in def".to_string(),
                span: Some(pair_to_source_span(&colon_pair)),
            })?;
            type_annotation = Some(build_type_expr(type_pair)?);
        }
    }

    let value_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "def requires a value expression".to_string(),
        span: Some(def_span),
    })?;
    let value = build_expression(value_pair)?;

    Ok(DefExpr {
        symbol,
        type_annotation,
        value: Box::new(value),
    })
}

pub(super) fn build_defn_expr(defn_expr_pair: Pair<Rule>) -> Result<DefnExpr, PestParseError> {
    let defn_span = pair_to_source_span(&defn_expr_pair);
    let mut pairs = defn_expr_pair.clone().into_inner();

    // Consume defn_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::defn_keyword {
            pairs.next();
        }
    }

    // Parse optional metadata before symbol name (new grammar: metadata comes after defn and before symbol)
    let mut delegation_hint: Option<DelegationHint> = None;
    let mut metadata: Option<HashMap<MapKey, Expression>> = None;
    while let Some(peek_pair) = pairs.peek() {
        match peek_pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
            }
            Rule::metadata => {
                let meta_pair = pairs.next().unwrap();
                let meta_span = pair_to_source_span(&meta_pair);
                // Determine if this is delegation metadata or general metadata
                let inner_pairs: Vec<_> = meta_pair.clone().into_inner().collect();
                if inner_pairs.len() == 1 && inner_pairs[0].as_rule() == Rule::delegation_meta {
                    let delegation_meta_pair = meta_pair
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::delegation_meta)
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "metadata must contain delegation_meta".to_string(),
                            span: Some(meta_span),
                        })?;
                    delegation_hint = Some(parse_delegation_meta(delegation_meta_pair)?);
                } else if inner_pairs.len() == 1 && inner_pairs[0].as_rule() == Rule::general_meta {
                    metadata = Some(parse_general_meta(meta_pair)?);
                }
            }
            _ => break,
        }
    }

    let symbol_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "defn requires a symbol (function name)".to_string(),
        span: Some(defn_span.clone()),
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput {
            message: format!(
                "Expected symbol for defn name, found {:?}",
                symbol_pair.as_rule()
            ),
            span: Some(pair_to_source_span(&symbol_pair)),
        });
    }
    let name = build_symbol(symbol_pair.clone())?;

    // Skip whitespace/comments before parameter list
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }

    let params_pair = pairs.next().unwrap();
    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput {
            message: format!(
                "Expected fn_param_list for defn, found {:?}",
                params_pair.as_rule()
            ),
            span: Some(pair_to_source_span(&params_pair)),
        });
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.clone().into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE
            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }
        if param_item_peek.as_rule() == Rule::AMPERSAND {
            let ampersand_pair = params_inner.next().unwrap();
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    params_inner.next();
                } else {
                    break;
                }
            }
            let rest_sym_pair =
                params_inner
                    .next()
                    .ok_or_else(|| PestParseError::InvalidInput {
                        message: "defn: & requires symbol".to_string(),
                        span: Some(pair_to_source_span(&ampersand_pair)),
                    })?;
            if rest_sym_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected symbol after &, found {:?}",
                        rest_sym_pair.as_rule()
                    ),
                    span: Some(pair_to_source_span(&rest_sym_pair)),
                });
            }
            let rest_sym = build_symbol(rest_sym_pair.clone())?;
            let mut rest_type: Option<TypeExpr> = None;
            if let Some(peek_colon) = params_inner.peek() {
                if peek_colon.as_rule() == Rule::COLON {
                    let colon_for_variadic_type_pair = params_inner.next().unwrap();
                    while let Some(p) = params_inner.peek() {
                        if p.as_rule() == Rule::WHITESPACE {
                            params_inner.next();
                        } else {
                            break;
                        }
                    }
                    let type_pair =
                        params_inner
                            .next()
                            .ok_or_else(|| PestParseError::InvalidInput {
                                message: "Expected type_expr after ':' for variadic parameter"
                                    .to_string(),
                                span: Some(pair_to_source_span(&colon_for_variadic_type_pair)),
                            })?;
                    rest_type = Some(build_type_expr(type_pair)?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_sym),
                type_annotation: rest_type,
            });
            break;
        }
        let param_def_pair = params_inner.next().unwrap();
        let param_def_span = pair_to_source_span(&param_def_pair);

        if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput {
                message: format!("Expected param_def, found {:?}", param_def_pair.as_rule()),
                span: Some(param_def_span.clone()),
            });
        }

        let mut param_def_inner = param_def_pair.clone().into_inner();

        let binding_pattern_pair =
            param_def_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "param_def missing binding_pattern".to_string(),
                    span: Some(param_def_span.clone()),
                })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        let mut type_ann = None;
        if let Some(colon_candidate_pair) = param_def_inner.next() {
            if colon_candidate_pair.as_rule() == Rule::COLON {
                let type_pair =
                    param_def_inner
                        .next()
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "Expected type_expr after ':' in param_def".to_string(),
                            span: Some(pair_to_source_span(&colon_candidate_pair)),
                        })?;
                type_ann = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected COLON in param_def, found {:?}",
                        colon_candidate_pair.as_rule()
                    ),
                    span: Some(pair_to_source_span(&colon_candidate_pair)),
                });
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
            let colon_for_return_type_pair = pairs.next().unwrap();
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    pairs.next();
                } else {
                    break;
                }
            }
            let ret_type_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
                message: "defn: expected return type after :".to_string(),
                span: Some(pair_to_source_span(&colon_for_return_type_pair)),
            })?;
            return_type = Some(build_type_expr(ret_type_pair)?);
        }
    }

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput {
            message: "defn requires at least one body expression".to_string(),
            span: Some(defn_span),
        });
    }

    Ok(DefnExpr {
        name,
        params,
        variadic_param,
        body,
        return_type,
        delegation_hint,
        metadata,
    })
}

pub(super) fn build_defstruct_expr(
    defstruct_expr_pair: Pair<Rule>,
) -> Result<DefstructExpr, PestParseError> {
    let defstruct_span = pair_to_source_span(&defstruct_expr_pair);
    let mut pairs = defstruct_expr_pair.clone().into_inner();

    // Consume defstruct_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::defstruct_keyword {
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

    let symbol_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "defstruct requires a symbol".to_string(),
        span: Some(defstruct_span.clone()),
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput {
            message: format!(
                "Expected symbol for defstruct, found {:?}",
                symbol_pair.as_rule()
            ),
            span: Some(pair_to_source_span(&symbol_pair)),
        });
    }
    let symbol = build_symbol(symbol_pair.clone())?;

    let mut fields = Vec::new();

    // Process field pairs (keyword type_expr keyword type_expr ...)
    while let Some(field_pair) = pairs.next() {
        if field_pair.as_rule() == Rule::WHITESPACE || field_pair.as_rule() == Rule::COMMENT {
            continue;
        }

        if field_pair.as_rule() == Rule::defstruct_field {
            let mut field_inner = field_pair.into_inner();

            let keyword_pair = field_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "defstruct_field missing keyword".to_string(),
                    span: Some(defstruct_span.clone()),
                })?;
            if keyword_pair.as_rule() != Rule::keyword {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected keyword in defstruct_field, found {:?}",
                        keyword_pair.as_rule()
                    ),
                    span: Some(pair_to_source_span(&keyword_pair)),
                });
            }
            let keyword = build_keyword(keyword_pair)?;

            let type_pair = field_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "defstruct_field missing type".to_string(),
                    span: Some(defstruct_span.clone()),
                })?;
            let field_type = build_type_expr(type_pair)?;

            fields.push(DefstructField {
                key: keyword,
                field_type,
            });
        } else {
            return Err(PestParseError::InvalidInput {
                message: format!("Expected defstruct_field, found {:?}", field_pair.as_rule()),
                span: Some(pair_to_source_span(&field_pair)),
            });
        }
    }

    Ok(DefstructExpr {
        name: symbol,
        fields,
    })
}

pub(super) fn build_try_catch_expr(
    try_catch_expr_pair: Pair<Rule>,
) -> Result<TryCatchExpr, PestParseError> {
    let try_catch_span = pair_to_source_span(&try_catch_expr_pair);
    let mut pairs = try_catch_expr_pair.clone().into_inner().peekable();

    // Consume try_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::try_keyword {
            pairs.next();
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let mut try_body_expressions = Vec::new();
    let mut last_try_expr_span = try_catch_span.clone(); // Fallback span

    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::catch_clause | Rule::finally_clause => break,
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
            }
            _ => {
                let expr_pair = pairs.next().unwrap();
                last_try_expr_span = pair_to_source_span(&expr_pair);
                try_body_expressions.push(build_expression(expr_pair)?);
            }
        }
    }

    // Allow empty try body. Semantics: evaluates to nil unless a catch handles an error or finally overrides via side effects.

    let mut catch_clauses = Vec::new();
    let mut finally_body: Option<Vec<Expression>> = None;
    let mut last_clause_end_span = last_try_expr_span.clone();

    while let Some(clause_candidate_peek) = pairs.peek() {
        let current_candidate_span = pair_to_source_span(clause_candidate_peek);
        match clause_candidate_peek.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
                continue;
            }
            Rule::catch_clause => {
                let catch_clause_pair = pairs.next().unwrap();
                let catch_clause_span = pair_to_source_span(&catch_clause_pair);
                last_clause_end_span = catch_clause_span.clone();
                let mut clause_inner = catch_clause_pair.into_inner().peekable();

                let _catch_keyword_pair = clause_inner
                    .next()
                    .filter(|p| p.as_rule() == Rule::catch_keyword)
                    .ok_or_else(|| PestParseError::InvalidInput {
                        message: "Catch clause missing 'catch' keyword".to_string(),
                        span: Some(catch_clause_span.clone()),
                    })?;

                let pattern_symbol_pair =
                    clause_inner
                        .next()
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "Catch clause requires at least one symbol after 'catch'"
                                .to_string(),
                            span: Some(catch_clause_span.clone()),
                        })?;
                if pattern_symbol_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::InvalidInput {
                        message: format!(
                            "Expected symbol for catch pattern, found {:?}",
                            pattern_symbol_pair.as_rule()
                        ),
                        span: Some(pair_to_source_span(&pattern_symbol_pair)),
                    });
                }
                let pattern = build_catch_pattern(pattern_symbol_pair.clone())?;
                let binding_symbol_pair = clause_inner.peek();
                let binding = if let Some(binding_symbol_pair) = binding_symbol_pair {
                    if binding_symbol_pair.as_rule() == Rule::symbol {
                        let binding_symbol_pair = clause_inner.next().unwrap();
                        build_symbol(binding_symbol_pair)?
                    } else {
                        // Only one symbol, use as both pattern and binding
                        match &pattern {
                            CatchPattern::Symbol(s) => s.clone(),
                            CatchPattern::Wildcard => {
                                return Err(PestParseError::InvalidInput {
                                    message: "Wildcard pattern requires a binding symbol"
                                        .to_string(),
                                    span: Some(pair_to_source_span(&pattern_symbol_pair)),
                                });
                            }
                            _ => {
                                return Err(PestParseError::InvalidInput {
                                    message: "Non-symbol pattern requires a binding symbol"
                                        .to_string(),
                                    span: Some(pair_to_source_span(&pattern_symbol_pair)),
                                });
                            }
                        }
                    }
                } else {
                    // Only one symbol, use as both pattern and binding
                    match &pattern {
                        CatchPattern::Symbol(s) => s.clone(),
                        CatchPattern::Wildcard => {
                            return Err(PestParseError::InvalidInput {
                                message: "Wildcard pattern requires a binding symbol".to_string(),
                                span: Some(pair_to_source_span(&pattern_symbol_pair)),
                            });
                        }
                        _ => {
                            return Err(PestParseError::InvalidInput {
                                message: "Non-symbol pattern requires a binding symbol".to_string(),
                                span: Some(pair_to_source_span(&pattern_symbol_pair)),
                            });
                        }
                    }
                };

                let mut catch_body_expressions = Vec::new();
                let mut last_catch_expr_span = pair_to_source_span(&pattern_symbol_pair);
                while let Some(body_expr_candidate) = clause_inner.next() {
                    match body_expr_candidate.as_rule() {
                        Rule::WHITESPACE | Rule::COMMENT => continue,
                        _ => {
                            last_catch_expr_span = pair_to_source_span(&body_expr_candidate);
                            catch_body_expressions.push(build_expression(body_expr_candidate)?);
                        }
                    }
                }
                if catch_body_expressions.is_empty() {
                    return Err(PestParseError::InvalidInput {
                        message: "Catch clause requires at least one body expression".to_string(),
                        span: Some(last_catch_expr_span),
                    });
                }
                catch_clauses.push(CatchClause {
                    pattern,
                    binding,
                    body: catch_body_expressions,
                });
            }
            Rule::finally_clause => {
                if finally_body.is_some() {
                    return Err(PestParseError::InvalidInput {
                        message: "Multiple finally clauses found".to_string(),
                        span: Some(current_candidate_span),
                    });
                }
                let finally_clause_pair = pairs.next().unwrap();
                let finally_clause_span = pair_to_source_span(&finally_clause_pair);
                last_clause_end_span = finally_clause_span.clone();
                let mut finally_inner = finally_clause_pair.into_inner().peekable();

                let _finally_keyword_pair = finally_inner
                    .next()
                    .filter(|p| p.as_rule() == Rule::finally_keyword)
                    .ok_or_else(|| PestParseError::InvalidInput {
                        message: "Finally clause missing 'finally' keyword".to_string(),
                        span: Some(finally_clause_span.clone()),
                    })?;

                let mut finally_expressions = Vec::new();
                let mut last_finally_expr_span = finally_clause_span.clone(); // Fallback
                while let Some(body_expr_candidate) = finally_inner.next() {
                    match body_expr_candidate.as_rule() {
                        Rule::WHITESPACE | Rule::COMMENT => continue,
                        _ => {
                            last_finally_expr_span = pair_to_source_span(&body_expr_candidate);
                            finally_expressions.push(build_expression(body_expr_candidate)?);
                        }
                    }
                }
                if finally_expressions.is_empty() {
                    return Err(PestParseError::InvalidInput {
                        message: "Finally clause requires at least one body expression".to_string(),
                        span: Some(last_finally_expr_span), // Span of keyword if body empty
                    });
                }
                finally_body = Some(finally_expressions);
            }
            _ => {
                return Err(PestParseError::InvalidInput {
                    message: format!(
                        "Expected catch_clause or finally_clause, found {:?} in try-catch",
                        clause_candidate_peek.as_rule()
                    ),
                    span: Some(current_candidate_span),
                });
            }
        }
    }

    if catch_clauses.is_empty() && finally_body.is_none() {
        return Err(PestParseError::InvalidInput {
            message: "try expression must have at least one catch clause or a finally clause"
                .to_string(),
            span: Some(last_clause_end_span), // Span of the last thing in the try block
        });
    }

    Ok(TryCatchExpr {
        try_body: try_body_expressions,
        catch_clauses,
        finally_body,
    })
}

// build_catch_pattern needs to align with AST CatchPattern and Pest catch_pattern rule
// catch_pattern  = _{ type_expr | keyword | symbol }
// AST: enum CatchPattern { Keyword(Keyword), Type(TypeExpr), Symbol(Symbol) }
fn build_catch_pattern(pair: Pair<Rule>) -> Result<CatchPattern, PestParseError> {
    let span = pair_to_source_span(&pair);
    match pair.as_rule() {
        Rule::type_expr => Ok(CatchPattern::Type(build_type_expr(pair.clone())?)),
        Rule::keyword => Ok(CatchPattern::Keyword(build_keyword(pair.clone())?)),
        Rule::symbol => {
            let symbol = build_symbol(pair.clone())?;
            if symbol.0 == "_" {
                Ok(CatchPattern::Wildcard)
            } else {
                Ok(CatchPattern::Symbol(symbol))
            }
        }
        Rule::primitive_type => Ok(CatchPattern::Symbol(build_symbol(pair.clone())?)),
        unknown_rule => Err(PestParseError::InvalidInput {
            message: format!(
                "Invalid rule for catch_pattern: {:?}, content: '{}'",
                unknown_rule,
                pair.as_str()
            ),
            span: Some(span),
        }),
    }
}

pub(super) fn build_match_expr(match_expr_pair: Pair<Rule>) -> Result<MatchExpr, PestParseError> {
    let match_span = pair_to_source_span(&match_expr_pair);
    let mut pairs = match_expr_pair.clone().into_inner().peekable();

    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::match_keyword | Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
            }
            _ => break,
        }
    }

    let expression_to_match_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "match expression requires an expression to match against".to_string(),
        span: Some(match_span.clone()),
    })?;
    let expression_to_match_span = pair_to_source_span(&expression_to_match_pair);
    let matched_expression = Box::new(build_expression(expression_to_match_pair)?);

    let mut clauses = Vec::new();
    while let Some(clause_candidate_pair) = pairs.next() {
        match clause_candidate_pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => continue,
            // Corrected to use Rule::match_clause_content, which is the actual rule in the grammar
            Rule::match_clause_content => {
                let clause_pair = clause_candidate_pair;
                let clause_span = pair_to_source_span(&clause_pair);
                let mut clause_inner = clause_pair.into_inner().peekable();

                while let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                        clause_inner.next();
                    } else {
                        break;
                    }
                }

                let pattern_pair =
                    clause_inner
                        .next()
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "match clause requires a pattern".to_string(),
                            span: Some(clause_span.clone()),
                        })?;
                let ast_pattern = build_match_pattern(pattern_pair)?; // Skip whitespace and comments
                while let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                        clause_inner.next();
                    } else {
                        break;
                    }
                }

                // Check for optional WHEN guard
                let mut guard_expr = None;
                if let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHEN {
                        clause_inner.next(); // consume WHEN token

                        // Skip whitespace and comments after WHEN
                        while let Some(p) = clause_inner.peek() {
                            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                                clause_inner.next();
                            } else {
                                break;
                            }
                        }

                        // Parse the guard expression
                        let guard_pair =
                            clause_inner
                                .next()
                                .ok_or_else(|| PestParseError::InvalidInput {
                                    message: "when clause requires a guard expression".to_string(),
                                    span: Some(clause_span.clone()),
                                })?;
                        guard_expr = Some(Box::new(build_expression(guard_pair)?));

                        // Skip whitespace and comments after guard
                        while let Some(p) = clause_inner.peek() {
                            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                                clause_inner.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                let expression_pair =
                    clause_inner
                        .next()
                        .ok_or_else(|| PestParseError::InvalidInput {
                            message: "match clause requires a body expression".to_string(),
                            span: Some(clause_span.clone()),
                        })?;
                let body_expr = Box::new(build_expression(expression_pair)?);

                clauses.push(MatchClause {
                    pattern: ast_pattern,
                    guard: guard_expr,
                    body: body_expr,
                });
            }
            unknown_rule => {
                return Err(PestParseError::InvalidInput { 
                    message: format!("Unexpected rule {:?} in match expression, expected Rule::match_clause_content", unknown_rule),
                    span: Some(pair_to_source_span(&clause_candidate_pair)) 
                });
            }
        }
    }

    if clauses.is_empty() {
        return Err(PestParseError::InvalidInput {
            message: "match expression requires at least one clause".to_string(),
            span: Some(expression_to_match_span),
        });
    }

    Ok(MatchExpr {
        expression: matched_expression,
        clauses,
    })
}

// Helper function to build MatchPattern from a Pair<Rule>
// This function is now implemented in super::common::build_match_pattern

// -----------------------------------------------------------------------------
// Metadata helpers
// -----------------------------------------------------------------------------

fn parse_delegation_meta(meta_pair: Pair<Rule>) -> Result<DelegationHint, PestParseError> {
    // Extract span information before moving the pair
    let meta_span = pair_to_source_span(&meta_pair);

    // Parse the structured pest pairs from the grammar
    let mut pairs = meta_pair.into_inner();

    // Find the delegation_target rule within the delegation_meta
    let delegation_target_pair = pairs
        .find(|p| p.as_rule() == Rule::delegation_target)
        .ok_or_else(|| PestParseError::InvalidInput {
            message: "delegation_meta must contain delegation_target".to_string(),
            span: Some(meta_span),
        })?;

    let delegation_target_span = pair_to_source_span(&delegation_target_pair);

    // Get the concrete delegation variant from delegation_target
    let mut target_inner = delegation_target_pair.into_inner();
    let concrete_pair = target_inner
        .next()
        .ok_or_else(|| PestParseError::InvalidInput {
            message: "delegation_target must contain a concrete delegation variant".to_string(),
            span: Some(delegation_target_span.clone()),
        })?;
    let concrete_span = pair_to_source_span(&concrete_pair);

    match concrete_pair.as_rule() {
        Rule::local_delegation => Ok(DelegationHint::LocalPure),

        Rule::local_model_delegation => {
            // Extract the required model id string
            let model_id_pair = concrete_pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::string)
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: ":local-model requires a string argument".to_string(),
                    span: Some(concrete_span.clone()),
                })?;

            let model_id = model_id_pair.as_str().trim_matches('"').to_string();
            Ok(DelegationHint::LocalModel(model_id))
        }

        Rule::remote_delegation => {
            // Extract the required remote model id string
            let remote_id_pair = concrete_pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::string)
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: ":remote requires a string argument".to_string(),
                    span: Some(concrete_span.clone()),
                })?;

            let remote_id = remote_id_pair.as_str().trim_matches('"').to_string();
            Ok(DelegationHint::RemoteModel(remote_id))
        }

        _ => Err(PestParseError::InvalidInput {
            message: format!(
                "Expected concrete delegation variant, found {:?}",
                concrete_pair.as_rule()
            ),
            span: Some(concrete_span),
        }),
    }
}

fn parse_general_meta(
    meta_pair: Pair<Rule>,
) -> Result<HashMap<MapKey, Expression>, PestParseError> {
    // Extract span information before moving the pair
    let meta_span = pair_to_source_span(&meta_pair);

    // Parse the structured pest pairs from the grammar
    let mut pairs = meta_pair.into_inner();

    // Skip the "^" and "{" and any whitespace/comments
    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
            }
            _ => break,
        }
    }

    // Get the general_meta rule content
    let general_meta_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "general_meta requires map content".to_string(),
        span: Some(meta_span),
    })?;

    // Parse the map entries
    let mut metadata = HashMap::new();
    let mut map_pairs = general_meta_pair.into_inner();

    while let Some(entry_pair) = map_pairs.next() {
        if entry_pair.as_rule() == Rule::map_entry {
            let entry_span = pair_to_source_span(&entry_pair);
            let mut entry_inner = entry_pair.into_inner();
            let key_pair = entry_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "map_entry requires key".to_string(),
                    span: Some(entry_span.clone()),
                })?;
            let value_pair = entry_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput {
                    message: "map_entry requires value".to_string(),
                    span: Some(entry_span),
                })?;

            let key = super::common::build_map_key(key_pair)?;
            let value = build_expression(value_pair)?;

            metadata.insert(key, value);
        }
    }

    Ok(metadata)
}

/// Build a plan expression from parsed pairs
// build_plan_expr removed: Plan is not a core special form in RTFS anymore.

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::DelegationHint;
    use crate::parser::RTFSParser;
    use pest::Parser;

    #[test]
    fn test_parse_delegation_meta_local() {
        let input = "^:delegation :local";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::delegation_meta, input).unwrap();
        let result = parse_delegation_meta(pairs.next().unwrap());
        assert_eq!(result.unwrap(), DelegationHint::LocalPure);
    }

    #[test]
    fn test_parse_delegation_meta_local_model() {
        let input = "^:delegation :local-model \"phi-mini\"";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::delegation_meta, input).unwrap();
        let result = parse_delegation_meta(pairs.next().unwrap());
        assert!(result.is_ok());
        if let Ok(DelegationHint::LocalModel(model_id)) = result {
            assert_eq!(model_id, "phi-mini");
        } else {
            panic!("Expected LocalModel delegation hint");
        }
    }

    #[test]
    fn test_parse_delegation_meta_remote() {
        let input = "^:delegation :remote \"gpt4o\"";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::delegation_meta, input).unwrap();
        let result = parse_delegation_meta(pairs.next().unwrap());
        assert_eq!(
            result.unwrap(),
            DelegationHint::RemoteModel("gpt4o".to_string())
        );
    }

    #[test]
    fn test_parse_delegation_meta_malformed() {
        let input = "^:delegation :local-model";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::delegation_meta, input).unwrap();
        let result = parse_delegation_meta(pairs.next().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_with_delegation_hint() {
        let input = "(fn ^:delegation :local-model \"phi-mini\" [x] (+ x 1))";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::fn_expr, input).unwrap();
        let result = build_fn_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let fn_expr = result.unwrap();
        assert_eq!(
            fn_expr.delegation_hint,
            Some(DelegationHint::LocalModel("phi-mini".to_string()))
        );
    }

    #[test]
    fn test_defn_with_delegation_hint() {
        let input = "(defn ^:delegation :remote \"gpt4o\" add [x y] (+ x y))";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::defn_expr, input).unwrap();
        let result = build_defn_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let defn_expr = result.unwrap();
        assert_eq!(
            defn_expr.delegation_hint,
            Some(DelegationHint::RemoteModel("gpt4o".to_string()))
        );
    }

    #[test]
    fn test_fn_without_delegation_hint() {
        let input = "(fn [x] (+ x 1))";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::fn_expr, input).unwrap();
        let result = build_fn_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let fn_expr = result.unwrap();
        assert_eq!(fn_expr.delegation_hint, None);
    }

    #[test]
    fn test_defn_without_delegation_hint() {
        let input = "(defn add [x y] (+ x y))";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::defn_expr, input).unwrap();
        let result = build_defn_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let defn_expr = result.unwrap();
        assert_eq!(defn_expr.delegation_hint, None);
    }

    #[test]
    fn test_defstruct_basic() {
        let input =
            "(defstruct GenerationContext :arbiter-version String :generation-timestamp Timestamp)";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::defstruct_expr, input).unwrap();
        let result = build_defstruct_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let defstruct_expr = result.unwrap();
        assert_eq!(defstruct_expr.name.0, "GenerationContext");
        assert_eq!(defstruct_expr.fields.len(), 2);
        assert_eq!(defstruct_expr.fields[0].key.0, "arbiter-version");
        assert_eq!(defstruct_expr.fields[1].key.0, "generation-timestamp");
    }

    #[test]
    fn test_defstruct_empty() {
        let input = "(defstruct EmptyStruct)";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::defstruct_expr, input).unwrap();
        let result = build_defstruct_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let defstruct_expr = result.unwrap();
        assert_eq!(defstruct_expr.name.0, "EmptyStruct");
        assert_eq!(defstruct_expr.fields.len(), 0);
    }

    #[test]
    fn test_defstruct_from_issue_example() {
        let input = "(defstruct GenerationContext :arbiter-version String :generation-timestamp Timestamp :input-context Any :reasoning-trace String)";
        let mut pairs = RTFSParser::parse(crate::parser::Rule::defstruct_expr, input).unwrap();
        let result = build_defstruct_expr(pairs.next().unwrap());
        assert!(result.is_ok());
        let defstruct_expr = result.unwrap();
        assert_eq!(defstruct_expr.name.0, "GenerationContext");
        assert_eq!(defstruct_expr.fields.len(), 4);
        assert_eq!(defstruct_expr.fields[0].key.0, "arbiter-version");
        assert_eq!(defstruct_expr.fields[1].key.0, "generation-timestamp");
        assert_eq!(defstruct_expr.fields[2].key.0, "input-context");
        assert_eq!(defstruct_expr.fields[3].key.0, "reasoning-trace");
    }

    #[test]
    fn test_defstruct_as_expression() {
        let input =
            "(defstruct GenerationContext :arbiter-version String :generation-timestamp Timestamp)";
        let result = crate::parser::parse_expression(input);
        assert!(result.is_ok());
        if let Ok(crate::ast::Expression::Defstruct(defstruct_expr)) = result {
            assert_eq!(defstruct_expr.name.0, "GenerationContext");
            assert_eq!(defstruct_expr.fields.len(), 2);
        } else {
            panic!("Expected defstruct expression");
        }
    }

    // Plan parsing test removed; plan is handled at CCOS layer (as FunctionCall/Map)
}
